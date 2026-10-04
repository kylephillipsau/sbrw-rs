package com.soapboxrace.reference;

import org.jboss.resteasy.core.Dispatcher;
import org.jboss.resteasy.mock.MockDispatcherFactory;
import org.jboss.resteasy.mock.MockHttpRequest;
import org.jboss.resteasy.mock.MockHttpResponse;
import org.jboss.resteasy.spi.ResteasyProviderFactory;

import javax.servlet.http.HttpServletRequest;
import javax.ws.rs.Path;
import javax.ws.rs.ext.Provider;
import java.lang.reflect.Proxy;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/**
 * Runs the original JAX-RS resources, filters, marshaller and exception mapper
 * in-process through RESTEasy's mock dispatcher, under {@code /Engine.svc}.
 */
public final class Http {
    private final Dispatcher dispatcher;
    private final Container container;
    private Scenario.HttpRequest current;

    public Http(Container container, List<Class<?>> coreClasses) {
        this.container = container;
        this.dispatcher = MockDispatcherFactory.createDispatcher();
        ResteasyProviderFactory providers = dispatcher.getProviderFactory();
        for (Class<?> c : coreClasses) {
            if (c.isAnnotationPresent(Provider.class)) {
                providers.registerProviderInstance(container.get(c));
            }
        }
        for (Class<?> c : coreClasses) {
            if (c.isAnnotationPresent(Path.class) && c.getPackage().getName().endsWith(".api")) {
                dispatcher.getRegistry().addSingletonResource(container.get(c), "/Engine.svc");
            }
        }
        // Resources and filters get @Context HttpServletRequest; point it at the current request.
        HttpServletRequest servletRequest = (HttpServletRequest) Proxy.newProxyInstance(
                HttpServletRequest.class.getClassLoader(), new Class<?>[]{HttpServletRequest.class},
                (proxy, method, args) -> servletCall(method.getName(), args, method.getReturnType()));
        dispatcher.getDefaultContextObjects().put(HttpServletRequest.class, servletRequest);
    }

    public Map<String, Object> invoke(Scenario.HttpRequest r) throws Exception {
        current = r;
        container.beginRequest();
        String uri = r.path + query(r.query);
        MockHttpRequest request = MockHttpRequest.create(r.method, uri);
        if (r.headers != null) {
            r.headers.forEach(request::header);
        }
        if (r.body != null) {
            request.content(r.body.getBytes(StandardCharsets.UTF_8));
            request.contentType(r.contentType != null ? r.contentType : "application/xml");
        } else if (r.form != null) {
            request.contentType("application/x-www-form-urlencoded");
            request.content(query(r.form).substring(1).getBytes(StandardCharsets.UTF_8));
        }
        MockHttpResponse response = new MockHttpResponse();
        dispatcher.invoke(request, response);
        container.drainAsync();

        Map<String, Object> out = new LinkedHashMap<>();
        out.put("status", response.getStatus());
        Map<String, Object> headers = new TreeMap<>(String.CASE_INSENSITIVE_ORDER);
        response.getOutputHeaders().forEach((k, v) -> headers.put(k, v.size() == 1 ? String.valueOf(v.get(0)) : v));
        out.put("headers", headers);
        out.put("body", response.getContentAsString());
        return out;
    }

    private static String query(Map<String, String> params) {
        if (params == null || params.isEmpty()) {
            return "";
        }
        StringBuilder sb = new StringBuilder("?");
        params.forEach((k, v) -> {
            if (sb.length() > 1) sb.append('&');
            sb.append(URLEncoder.encode(k, StandardCharsets.UTF_8)).append('=')
                    .append(URLEncoder.encode(v, StandardCharsets.UTF_8));
        });
        return sb.toString();
    }

    private Object servletCall(String name, Object[] args, Class<?> returnType) {
        Scenario.HttpRequest r = current;
        switch (name) {
            case "getParameter": {
                String key = (String) args[0];
                if (r.query != null && r.query.containsKey(key)) return r.query.get(key);
                if (r.form != null && r.form.containsKey(key)) return r.form.get(key);
                return null;
            }
            case "getHeader": {
                if (r.headers == null) return null;
                for (Map.Entry<String, String> e : r.headers.entrySet()) {
                    if (e.getKey().equalsIgnoreCase((String) args[0])) return e.getValue();
                }
                return null;
            }
            case "getRemoteAddr":
            case "getRemoteHost":
                return r.remoteAddr != null ? r.remoteAddr : "127.0.0.1";
            case "getMethod":
                return r.method;
            case "getRequestURI":
                return r.path;
            case "getQueryString":
                return query(r.query).isEmpty() ? null : query(r.query).substring(1);
            case "hashCode":
                return System.identityHashCode(this);
            case "equals":
                return args[0] == this;
            case "toString":
                return "HttpServletRequest(" + r.method + " " + r.path + ")";
            default:
                if (returnType == boolean.class) return false;
                if (returnType == int.class || returnType == long.class) return 0;
                return null;
        }
    }
}
