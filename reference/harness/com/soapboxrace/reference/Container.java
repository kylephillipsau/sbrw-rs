package com.soapboxrace.reference;

import com.soapboxrace.core.bo.RequestSessionInfo;
import net.bytebuddy.ByteBuddy;
import net.bytebuddy.dynamic.loading.ClassLoadingStrategy;
import net.bytebuddy.implementation.MethodDelegation;
import net.bytebuddy.implementation.bind.annotation.AllArguments;
import net.bytebuddy.implementation.bind.annotation.Origin;
import net.bytebuddy.implementation.bind.annotation.RuntimeType;
import org.slf4j.LoggerFactory;

import javax.annotation.PostConstruct;
import javax.annotation.Resource;
import javax.ejb.ApplicationException;
import javax.ejb.Asynchronous;
import javax.ejb.EJB;
import javax.ejb.EJBException;
import javax.ejb.EJBTransactionRolledbackException;
import javax.ejb.Schedule;
import javax.ejb.Singleton;
import javax.ejb.Startup;
import javax.ejb.Stateless;
import javax.ejb.Timeout;
import javax.ejb.Timer;
import javax.ejb.TimerConfig;
import javax.ejb.TimerService;
import javax.enterprise.event.Event;
import javax.enterprise.event.Observes;
import javax.inject.Inject;
import javax.persistence.EntityManager;
import javax.persistence.EntityManagerFactory;
import javax.persistence.PersistenceContext;
import javax.persistence.TransactionRequiredException;
import java.lang.invoke.MethodHandles;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.lang.reflect.Parameter;
import java.lang.reflect.Proxy;
import java.time.Instant;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.Deque;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Properties;
import java.util.concurrent.Callable;

import static net.bytebuddy.matcher.ElementMatchers.isDeclaredBy;
import static net.bytebuddy.matcher.ElementMatchers.isFinal;
import static net.bytebuddy.matcher.ElementMatchers.isPublic;
import static net.bytebuddy.matcher.ElementMatchers.isStatic;
import static net.bytebuddy.matcher.ElementMatchers.not;

/**
 * A minimal stand-in for the Java EE container the core was written for.
 *
 * <p>It reproduces the semantics the port has to match: one instance per bean;
 * EJB business methods with REQUIRED transactions (each call from the non-EJB
 * JAX-RS resources commits on its own); application exceptions such as
 * {@code EngineException} committing, system exceptions rolling back and arriving
 * wrapped in {@code EJBException}; transaction-scoped persistence contexts;
 * {@code @Asynchronous} methods running after the current step; CDI events; and
 * single-action timers that fire only when a scenario says so.
 */
public final class Container {
    private final EntityManagerFactory emf;
    private final Recording recording;
    private final List<Class<?>> coreClasses;
    private final Map<Class<?>, Class<?>> substitutes;
    private final Map<Class<?>, Object> beans = new HashMap<>();
    private final Map<Class<?>, Object> targets = new HashMap<>();
    private final Deque<Callable<Object>> asyncQueue = new ArrayDeque<>();
    private final List<PendingTimer> timers = new ArrayList<>();
    private final EntityManager entityManagerProxy;
    private EntityManager currentEm;
    private boolean rollbackOnly;
    private RequestSessionInfo requestSession = new RequestSessionInfo();
    private long timerSequence;

    public Container(EntityManagerFactory emf, Recording recording, List<Class<?>> coreClasses,
                     Map<Class<?>, Class<?>> substitutes) {
        this.emf = emf;
        this.recording = recording;
        this.coreClasses = coreClasses;
        this.substitutes = substitutes;
        this.entityManagerProxy = (EntityManager) Proxy.newProxyInstance(EntityManager.class.getClassLoader(),
                new Class<?>[]{EntityManager.class}, (proxy, method, args) -> {
                    if (currentEm == null) {
                        throw new TransactionRequiredException("no active transaction for " + method.getName());
                    }
                    return invoke(method, currentEm, args);
                });
    }

    public Recording recording() {
        return recording;
    }

    /** The bean instance itself, behind its proxy: for calling a private method, as the bean's own code does. */
    Object target(Class<?> type) {
        get(type);
        return targets.get(type);
    }

    /** Eagerly create {@code @Startup} singletons, as the server does at boot. */
    public void boot() throws Exception {
        List<Class<?>> startup = new ArrayList<>();
        for (Class<?> c : coreClasses) {
            if (c.isAnnotationPresent(Startup.class)) {
                startup.add(c);
            }
        }
        startup.sort(Comparator.comparing(Class::getName));
        for (Class<?> c : startup) {
            get(c);
        }
        drainAsync();
    }

    /** Fresh request-scoped state, as CDI creates per HTTP request. */
    public void beginRequest() {
        requestSession = new RequestSessionInfo();
    }

    @SuppressWarnings("unchecked")
    public <T> T get(Class<T> type) {
        Object existing = beans.get(type);
        if (existing != null) {
            return (T) existing;
        }
        Class<?> impl = substitutes.getOrDefault(type, type);
        Object instance;
        try {
            if (type == RequestSessionInfo.class) {
                instance = newDelegatingProxy(RequestSessionInfo.class);
                beans.put(type, instance);
                return (T) instance;
            }
            Object target = newInstance(impl);
            targets.put(type, target);
            instance = isEjb(type) ? newTransactionalProxy(impl, target) : target;
            // Other beans get the proxy; the bean's own calls on `this` reach the
            // target directly, so they skip the container, as on a real server.
            beans.put(type, instance);
            inject(target, impl);
            postConstruct(target, impl);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("cannot create bean " + type.getName(), e);
        }
        return (T) instance;
    }

    private static boolean isEjb(Class<?> type) {
        return type.isAnnotationPresent(Stateless.class) || type.isAnnotationPresent(Singleton.class);
    }

    private static Object newInstance(Class<?> impl) throws ReflectiveOperationException {
        var constructor = impl.getDeclaredConstructor();
        constructor.setAccessible(true);
        return constructor.newInstance();
    }

    /** A proxy that forwards business methods to {@code target} inside the container's rules. */
    private Object newTransactionalProxy(Class<?> impl, Object target) throws ReflectiveOperationException {
        Class<?> proxyClass = new ByteBuddy()
                .subclass(impl)
                .method(isPublic().and(not(isStatic())).and(not(isFinal())).and(not(isDeclaredBy(Object.class))))
                .intercept(MethodDelegation.to(new EjbInterceptor(this, target)))
                .make()
                .load(impl.getClassLoader(), ClassLoadingStrategy.UsingLookup.of(
                        MethodHandles.privateLookupIn(impl, MethodHandles.lookup())))
                .getLoaded();
        return newInstance(proxyClass);
    }

    /** A CDI-style client proxy whose calls go to the current request's instance. */
    private Object newDelegatingProxy(Class<?> type) throws ReflectiveOperationException {
        Class<?> proxyClass = new ByteBuddy()
                .subclass(type)
                .method(isPublic().and(not(isStatic())).and(not(isDeclaredBy(Object.class))))
                .intercept(MethodDelegation.to(new RequestScopeInterceptor(this)))
                .make()
                .load(type.getClassLoader(), ClassLoadingStrategy.UsingLookup.of(
                        MethodHandles.privateLookupIn(type, MethodHandles.lookup())))
                .getLoaded();
        return newInstance(proxyClass);
    }

    private void inject(Object instance, Class<?> impl) {
        for (Class<?> c = impl; c != null && c != Object.class; c = c.getSuperclass()) {
            for (Field f : c.getDeclaredFields()) {
                Object value = null;
                if (f.isAnnotationPresent(EJB.class)) {
                    value = get(f.getType());
                } else if (f.isAnnotationPresent(Inject.class)) {
                    value = injectable(f.getType(), impl);
                } else if (f.isAnnotationPresent(PersistenceContext.class)) {
                    value = entityManagerProxy;
                } else if (f.isAnnotationPresent(Resource.class)) {
                    value = resource(f.getType(), impl);
                }
                if (value != null) {
                    set(f, instance, value);
                }
            }
            for (Method m : c.getDeclaredMethods()) {
                if (m.isAnnotationPresent(PersistenceContext.class)) {
                    // A plain setter: call the original, not the transactional override.
                    Field emField = findEntityManagerField(c);
                    set(emField, instance, entityManagerProxy);
                }
            }
        }
    }

    private static Field findEntityManagerField(Class<?> c) {
        for (Field f : c.getDeclaredFields()) {
            if (f.getType() == EntityManager.class) {
                return f;
            }
        }
        throw new IllegalStateException(c.getName() + " has a @PersistenceContext setter but no EntityManager field");
    }

    private Object injectable(Class<?> type, Class<?> owner) {
        if (type == org.slf4j.Logger.class) {
            return LoggerFactory.getLogger(owner);
        }
        if (type == Event.class) {
            return Proxy.newProxyInstance(Event.class.getClassLoader(), new Class<?>[]{Event.class},
                    (proxy, method, args) -> {
                        if (method.getName().equals("fire")) {
                            fireEvent(args[0]);
                            return null;
                        }
                        throw new UnsupportedOperationException("Event." + method.getName());
                    });
        }
        return get(type);
    }

    private Object resource(Class<?> type, Class<?> owner) {
        if (type == TimerService.class) {
            return timerService(owner);
        }
        if (type == javax.mail.Session.class) {
            // No SMTP in the reference; sending fails the way an unreachable server would.
            Properties p = new Properties();
            p.put("mail.smtp.host", "127.0.0.1");
            p.put("mail.smtp.port", "1");
            return javax.mail.Session.getInstance(p);
        }
        throw new IllegalStateException("unsupported @Resource " + type.getName() + " in " + owner.getName());
    }

    private void postConstruct(Object instance, Class<?> impl) {
        List<Method> hooks = new ArrayList<>();
        for (Class<?> c = impl; c != null && c != Object.class; c = c.getSuperclass()) {
            for (Method m : c.getDeclaredMethods()) {
                if (m.isAnnotationPresent(PostConstruct.class)) {
                    hooks.add(0, m);
                }
            }
        }
        for (Method m : hooks) {
            try {
                m.setAccessible(true);
                // Singletons' @PostConstruct runs in a REQUIRED transaction.
                inTransaction(() -> m.invoke(instance));
            } catch (Exception e) {
                throw new IllegalStateException("@PostConstruct " + impl.getSimpleName() + "." + m.getName() + " failed",
                        unwrap(e));
            }
        }
    }

    // ---- transactions -------------------------------------------------------------------------

    Object inTransaction(Callable<Object> body) throws Exception {
        if (currentEm != null) {
            try {
                return body.call();
            } catch (RuntimeException e) {
                RuntimeException cause = e;
                if (isApplicationException(cause)) {
                    if (rollsBack(cause)) {
                        rollbackOnly = true;
                    }
                    throw cause;
                }
                rollbackOnly = true;
                throw cause instanceof EJBException ? cause
                        : new EJBTransactionRolledbackException(cause.toString(), toException(cause));
            }
        }
        EntityManager em = emf.createEntityManager();
        currentEm = em;
        rollbackOnly = false;
        em.getTransaction().begin();
        pinDatabaseClock(em);
        try {
            Object result;
            try {
                result = body.call();
            } catch (RuntimeException e) {
                RuntimeException cause = e;
                boolean commit = isApplicationException(cause) && !rollsBack(cause) && !rollbackOnly;
                finish(em, commit);
                if (isApplicationException(cause)) {
                    throw cause;
                }
                throw cause instanceof EJBException ? cause : new EJBException(toException(cause));
            }
            if (rollbackOnly) {
                finish(em, false);
                throw new EJBTransactionRolledbackException("Transaction rolled back");
            }
            try {
                finish(em, true);
            } catch (RuntimeException e) {
                throw new EJBTransactionRolledbackException(e.toString(), toException(e));
            }
            return result;
        } finally {
            if (currentEm == em) {
                currentEm = null;
            }
            if (em.isOpen()) {
                em.close();
            }
        }
    }

    /**
     * The source patches can't reach SQL: four named queries compare against
     * {@code CURRENT_TIMESTAMP} and two columns default to it. Point MySQL's
     * session clock at the reference clock for every transaction (pooled
     * connections keep the setting, so it must be refreshed each time).
     */
    private static void pinDatabaseClock(EntityManager em) {
        em.unwrap(org.hibernate.Session.class).doWork(connection -> {
            try (java.sql.Statement st = connection.createStatement()) {
                st.execute("SET TIMESTAMP = " + Determinism.millis() / 1000.0);
            }
        });
    }

    private void finish(EntityManager em, boolean commit) {
        try {
            if (commit) {
                em.getTransaction().commit();
            } else if (em.getTransaction().isActive()) {
                em.getTransaction().rollback();
            }
        } finally {
            currentEm = null;
            em.close();
        }
    }

    private static boolean isApplicationException(Throwable t) {
        return !(t instanceof RuntimeException) || applicationException(t.getClass()) != null;
    }

    private static boolean rollsBack(Throwable t) {
        ApplicationException a = applicationException(t.getClass());
        return a != null && a.rollback();
    }

    private static ApplicationException applicationException(Class<?> c) {
        for (; c != null; c = c.getSuperclass()) {
            ApplicationException a = c.getAnnotation(ApplicationException.class);
            if (a != null) {
                return a;
            }
        }
        return null;
    }

    private static Exception toException(Throwable t) {
        return t instanceof Exception ? (Exception) t : new RuntimeException(t);
    }

    // ---- async, events, timers ----------------------------------------------------------------

    void enqueueAsync(Callable<Object> call) {
        asyncQueue.add(call);
    }

    /** Run queued {@code @Asynchronous} invocations, each in its own transaction. */
    public void drainAsync() throws Exception {
        while (!asyncQueue.isEmpty()) {
            Callable<Object> call = asyncQueue.poll();
            try {
                inTransaction(call);
            } catch (RuntimeException e) {
                // Async failures never reach the caller; the container just logs them.
                LoggerFactory.getLogger(Container.class).warn("@Asynchronous call failed", e);
            }
        }
    }

    private void fireEvent(Object event) throws Exception {
        for (Class<?> c : coreClasses) {
            for (Method m : c.getDeclaredMethods()) {
                Parameter[] ps = m.getParameters();
                if (ps.length == 1 && ps[0].isAnnotationPresent(Observes.class)
                        && ps[0].getType().isAssignableFrom(event.getClass())) {
                    Object bean = get(c);
                    m.setAccessible(true);
                    invoke(m, bean, new Object[]{event});
                }
            }
        }
    }

    private TimerService timerService(Class<?> owner) {
        return (TimerService) Proxy.newProxyInstance(TimerService.class.getClassLoader(),
                new Class<?>[]{TimerService.class}, (proxy, method, args) -> {
                    if (method.getName().equals("createSingleActionTimer") && args[0] instanceof Long
                            && args[1] instanceof TimerConfig) {
                        long delay = (Long) args[0];
                        Object info = ((TimerConfig) args[1]).getInfo();
                        timers.add(new PendingTimer(owner, Determinism.now().plusMillis(delay), info, timerSequence++));
                        recording.timer(owner.getSimpleName(), delay, info);
                        return timer(info);
                    }
                    throw new UnsupportedOperationException("TimerService." + method.getName());
                });
    }

    private static Timer timer(Object info) {
        return (Timer) Proxy.newProxyInstance(Timer.class.getClassLoader(), new Class<?>[]{Timer.class},
                (proxy, method, args) -> {
                    if (method.getName().equals("getInfo")) {
                        return info;
                    }
                    throw new UnsupportedOperationException("Timer." + method.getName());
                });
    }

    /** Fire every timer due at the current reference time, earliest first. */
    public int fireDueTimers() throws Exception {
        int fired = 0;
        while (true) {
            PendingTimer next = null;
            for (PendingTimer t : timers) {
                if (!t.due.isAfter(Determinism.now())
                        && (next == null || t.due.isBefore(next.due) || (t.due.equals(next.due) && t.seq < next.seq))) {
                    next = t;
                }
            }
            if (next == null) {
                return fired;
            }
            timers.remove(next);
            Method timeout = findAnnotated(next.bean, Timeout.class);
            Object bean = get(next.bean);
            invoke(timeout, bean, new Object[]{timer(next.info)});
            drainAsync();
            fired++;
        }
    }

    /** Invoke a scheduled job such as {@code BanBO#checkExpiredBans}. */
    public void runJob(String spec) throws Exception {
        String[] parts = spec.split("#");
        Class<?> c = coreClasses.stream().filter(k -> k.getSimpleName().equals(parts[0])).findFirst()
                .orElseThrow(() -> new IllegalArgumentException("no class " + parts[0]));
        Method m = c.getMethod(parts[1]);
        if (!m.isAnnotationPresent(Schedule.class)) {
            throw new IllegalArgumentException(spec + " is not a @Schedule method");
        }
        invoke(m, get(c), new Object[0]);
        drainAsync();
    }

    private static Method findAnnotated(Class<?> c, Class<? extends java.lang.annotation.Annotation> a) {
        for (Method m : c.getDeclaredMethods()) {
            if (m.isAnnotationPresent(a)) {
                m.setAccessible(true);
                return m;
            }
        }
        throw new IllegalStateException(c.getName() + " has no @" + a.getSimpleName());
    }

    // ---- helpers ------------------------------------------------------------------------------

    private static Object invoke(Method m, Object target, Object[] args) throws Exception {
        try {
            return m.invoke(target, args);
        } catch (InvocationTargetException e) {
            Throwable cause = e.getCause();
            if (cause instanceof Exception) {
                throw (Exception) cause;
            }
            throw (Error) cause;
        }
    }

    static Throwable unwrap(Throwable e) {
        while (e instanceof InvocationTargetException && e.getCause() != null) {
            e = e.getCause();
        }
        return e;
    }

    private static void set(Field f, Object target, Object value) {
        try {
            if (Modifier.isStatic(f.getModifiers())) {
                return;
            }
            f.setAccessible(true);
            f.set(target, value);
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    private static final class PendingTimer {
        final Class<?> bean;
        final Instant due;
        final Object info;
        final long seq;

        PendingTimer(Class<?> bean, Instant due, Object info, long seq) {
            this.bean = bean;
            this.due = due;
            this.info = info;
            this.seq = seq;
        }
    }

    /** Forwards EJB business methods to the bean, inside container-managed transactions. */
    public static final class EjbInterceptor {
        private final Container container;
        private final Object target;

        EjbInterceptor(Container container, Object target) {
            this.container = container;
            this.target = target;
        }

        @RuntimeType
        public Object intercept(@Origin Method method, @AllArguments Object[] args) throws Exception {
            method.setAccessible(true);
            Callable<Object> call = () -> invoke(method, target, args);
            if (method.isAnnotationPresent(Asynchronous.class)) {
                container.enqueueAsync(call);
                return null;
            }
            return container.inTransaction(call);
        }
    }

    /** Forwards calls on the injected RequestSessionInfo to the current request's instance. */
    public static final class RequestScopeInterceptor {
        private final Container container;

        RequestScopeInterceptor(Container container) {
            this.container = container;
        }

        @RuntimeType
        public Object intercept(@Origin Method method, @AllArguments Object[] args) throws Exception {
            return invoke(method, container.requestSession, args);
        }
    }
}
