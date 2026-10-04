package com.soapboxrace.reference;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.soapboxrace.jaxb.util.JAXBUtility;
import com.sun.xml.bind.v2.model.core.PropertyKind;
import com.sun.xml.bind.v2.model.runtime.RuntimeAttributePropertyInfo;
import com.sun.xml.bind.v2.model.runtime.RuntimeClassInfo;
import com.sun.xml.bind.v2.model.runtime.RuntimeElementPropertyInfo;
import com.sun.xml.bind.v2.model.runtime.RuntimeEnumLeafInfo;
import com.sun.xml.bind.v2.model.runtime.RuntimeNonElement;
import com.sun.xml.bind.v2.model.runtime.RuntimePropertyInfo;
import com.sun.xml.bind.v2.model.runtime.RuntimeTypeInfoSet;
import com.sun.xml.bind.v2.model.runtime.RuntimeTypeRef;
import com.sun.xml.bind.v2.model.runtime.RuntimeValuePropertyInfo;
import com.sun.xml.bind.v2.runtime.JAXBContextImpl;

import javax.xml.bind.JAXBContext;
import javax.xml.bind.annotation.XmlRootElement;
import javax.xml.bind.annotation.XmlSchemaType;
import javax.xml.bind.annotation.XmlTransient;
import javax.xml.bind.annotation.XmlType;
import javax.xml.datatype.DatatypeFactory;
import javax.xml.datatype.XMLGregorianCalendar;
import java.io.Writer;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.Type;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.stream.Collectors;
import java.util.stream.Stream;

/**
 * The XML classes (com.soapboxrace.jaxb.*) as JAXB itself sees them, for
 * tools/gen.py to generate Rust from, plus golden XML to test that Rust
 * against:
 * <ul>
 * <li>fixtures/dto/schema.json: every class's root name, base class and
 * properties in marshalling order (kind, element or attribute name, type,
 * list/wrapper, default value), and every enum's constants;</li>
 * <li>fixtures/dto/golden.json: per class, JAXBUtility.marshal of a fully
 * populated instance and of {@code new X()}.</li>
 * </ul>
 */
public final class Dtos {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().serializeNulls().create();
    private static final float[] FLOATS = {0.0f, 1.0f, -2.5f, 12.375f, 1.0E7f, 1.2345678E-4f, 0.1f, 3.4028235E38f,
            123456.78f, -0.001f, 650.0f, 9999999.0f};
    private static final double[] DOUBLES = {0.1, 1.0E7, 2.5E-5, -123.456, 1.0 / 3.0, 0.001, 9999999.0, 100.0,
            350000.0, 1.0E-4};

    public static void main(String[] args) throws Exception {
        Path out = Paths.get(args[0]).resolve("fixtures/dto");
        Files.createDirectories(out);
        List<Class<?>> classes = dtoClasses();
        // One context per class, as JAXBUtility does: some classes in different
        // packages share an XML type name and can't be in one context.
        Map<String, RuntimeClassInfo> beans = new TreeMap<>();
        Map<String, RuntimeEnumLeafInfo> enums = new TreeMap<>();
        for (Class<?> c : classes) {
            RuntimeTypeInfoSet model = ((JAXBContextImpl) JAXBContext.newInstance(c)).getTypeInfoSet();
            for (RuntimeClassInfo bean : model.beans().values()) {
                if (bean.getClazz().getName().startsWith("com.soapboxrace.")) beans.putIfAbsent(bean.getClazz().getName(), bean);
            }
            for (RuntimeEnumLeafInfo e : model.enums().values()) enums.putIfAbsent(e.getClazz().getName(), e);
        }

        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("about", "JAXB runtime model of com.soapboxrace.jaxb.* (" + CoreClasses.UPSTREAM + ")");
        List<Map<String, Object>> classOut = new ArrayList<>();
        Map<String, Map<String, String>> golden = new TreeMap<>();
        for (RuntimeClassInfo bean : beans.values()) {
            Class<?> c = bean.getClazz();
            classOut.add(describe(bean));
            if (Modifier.isAbstract(c.getModifiers())) continue;
            Map<String, String> g = new LinkedHashMap<>();
            g.put("sample", JAXBUtility.marshal(fill(c, 0)));
            g.put("empty", JAXBUtility.marshal(newInstance(c)));
            golden.put(c.getName(), g);
        }
        schema.put("classes", classOut);

        List<Map<String, Object>> enumOut = new ArrayList<>();
        for (RuntimeEnumLeafInfo e : enums.values()) {
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("java", e.getClazz().getName());
            List<Map<String, String>> constants = new ArrayList<>();
            for (var constant : e.getConstants()) {
                Map<String, String> k = new LinkedHashMap<>();
                k.put("name", constant.getName());
                k.put("xml", constant.getLexicalValue());
                constants.add(k);
            }
            m.put("constants", constants);
            enumOut.add(m);
        }
        schema.put("enums", enumOut);

        write(out.resolve("schema.json"), schema);
        write(out.resolve("golden.json"), golden);
        write(out.resolve("unmarshal.json"), unmarshalProbes());
    }

    private static List<Class<?>> dtoClasses() throws Exception {
        Path root = Paths.get(Dtos.class.getProtectionDomain().getCodeSource().getLocation().toURI());
        List<Class<?>> classes = new ArrayList<>();
        // The jaxb packages, plus the few XML classes that live elsewhere in the core.
        for (String pkg : new String[]{"jaxb/http", "jaxb/xmpp", "jaxb/login", "core/engine", "core/xmpp"}) {
            Path dir = root.resolve("com/soapboxrace/" + pkg);
            try (Stream<Path> s = Files.list(dir)) {
                for (String n : s.map(p -> p.getFileName().toString()).filter(n -> n.endsWith(".class")).sorted()
                        .collect(Collectors.toList())) {
                    Class<?> c = Class.forName("com.soapboxrace." + pkg.replace('/', '.') + "." + n.replace(".class", ""));
                    if (c.isAnnotationPresent(XmlType.class) || c.isAnnotationPresent(XmlRootElement.class)) classes.add(c);
                }
            }
        }
        return classes;
    }

    private static Map<String, Object> describe(RuntimeClassInfo bean) throws Exception {
        Class<?> c = bean.getClazz();
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("java", c.getName());
        // JAXBUtility.marshal: the @XmlRootElement name, else the @XmlType name.
        m.put("root", bean.isElement() ? bean.getElementName().getLocalPart() : c.getAnnotation(XmlType.class).name());
        m.put("base", bean.getBaseClass() == null ? null : bean.getBaseClass().getClazz().getName());
        m.put("abstract", Modifier.isAbstract(c.getModifiers()));
        Object instance = Modifier.isAbstract(c.getModifiers()) ? null : newInstance(c);
        List<Map<String, Object>> props = new ArrayList<>();
        for (RuntimePropertyInfo p : bean.getProperties()) {
            Map<String, Object> pm = new LinkedHashMap<>();
            pm.put("name", p.getName());
            pm.put("collection", p.isCollection());
            Field f = field(c, p.getName());
            pm.put("java_type", f.getGenericType().getTypeName());
            XmlSchemaType st = f.getAnnotation(XmlSchemaType.class);
            pm.put("schema_type", st == null ? null : st.name());
            if (p.kind() == PropertyKind.ELEMENT) {
                RuntimeElementPropertyInfo ep = (RuntimeElementPropertyInfo) p;
                pm.put("kind", "element");
                List<Map<String, Object>> types = new ArrayList<>();
                for (RuntimeTypeRef tr : ep.getTypes()) {
                    Map<String, Object> t = new LinkedHashMap<>();
                    t.put("xml", tr.getTagName().getLocalPart());
                    t.put("type", typeName(tr.getTarget()));
                    t.put("nillable", tr.isNillable());
                    types.add(t);
                }
                pm.put("types", types);
                pm.put("wrapper", ep.getXmlName() == null ? null : ep.getXmlName().getLocalPart());
            } else if (p.kind() == PropertyKind.ATTRIBUTE) {
                RuntimeAttributePropertyInfo ap = (RuntimeAttributePropertyInfo) p;
                pm.put("kind", "attribute");
                pm.put("xml", ap.getXmlName().getLocalPart());
                pm.put("type", typeName(ap.getTarget()));
            } else if (p.kind() == PropertyKind.VALUE) {
                pm.put("kind", "value");
                pm.put("type", typeName(((RuntimeValuePropertyInfo) p).getTarget()));
            } else {
                throw new IllegalStateException(c.getName() + "." + p.getName() + ": unsupported property kind " + p.kind());
            }
            f.setAccessible(true);
            pm.put("default", instance == null ? null : defaultValue(f.get(instance)));
            props.add(pm);
        }
        m.put("properties", props);
        return m;
    }

    /**
     * How JAXBUtility.unMarshal reads what clients send: each probe is parsed as
     * a class and marshalled back, or reports the exception.
     */
    private static List<Map<String, String>> unmarshalProbes() {
        String login = "com.soapboxrace.jaxb.login.LoginStatusVO";
        String user = "com.soapboxrace.jaxb.http.User";
        String profile = "com.soapboxrace.jaxb.http.ProfileData";
        String badges = "com.soapboxrace.jaxb.http.BadgeBundle";
        String[][] probes = {
                {login, "<LoginStatusVO><UserId>12</UserId><LoginToken>t</LoginToken></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId> 12 </UserId></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId>\n\t12\n</UserId></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId>abc</UserId><LoginToken>t</LoginToken></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId>12.5</UserId></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId>+12</UserId></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId></UserId><LoginToken/></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId/></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId>99999999999999999999</UserId></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId>1</UserId><UserId>2</UserId></LoginStatusVO>"},
                {login, "<LoginStatusVO><Foo>1</Foo><UserId>3</UserId><Bar><Baz/></Bar></LoginStatusVO>"},
                {login, "<LoginStatusVO xmlns=\"http://schemas.datacontract.org/2004/07/Victory.DataLayer.Serialization\" xmlns:i=\"http://www.w3.org/2001/XMLSchema-instance\"><UserId>3</UserId><LoginToken i:nil=\"true\"/></LoginStatusVO>"},
                {login, "<a:LoginStatusVO xmlns:a=\"urn:x\"><a:UserId>4</a:UserId></a:LoginStatusVO>"},
                {login, "<Whatever><UserId>5</UserId></Whatever>"},
                {login, "<LoginStatusVO><Description><![CDATA[a<b & c]]></Description><LoginToken>x &amp; y &#169; &#x41;</LoginToken></LoginStatusVO>"},
                {login, "<LoginStatusVO><Description>  keep  spaces \r\n </Description></LoginStatusVO>"},
                {login, "<LoginStatusVO><Description>a<b>bold</b>c</Description></LoginStatusVO>"},
                {login, "<?xml version=\"1.0\" encoding=\"utf-8\"?><LoginStatusVO><UserId>6</UserId></LoginStatusVO>"},
                {login, "\uFEFF<LoginStatusVO><UserId>7</UserId></LoginStatusVO>"},
                {login, "<LoginStatusVO><UserId>8</UserId>"},
                {login, ""},
                {login, "<LoginStatusVO><Ban><Reason>r</Reason></Ban><Ban><Expires>e</Expires></Ban></LoginStatusVO>"},
                {user, "<User><fullGameAccess>true</fullGameAccess><isComplete>1</isComplete><subscribeMsg>TRUE</subscribeMsg></User>"},
                {user, "<User><fullGameAccess> false </fullGameAccess><isComplete>0</isComplete><subscribeMsg>yes</subscribeMsg></User>"},
                {user, "<User><remoteUserId>-5</remoteUserId><userId>abc</userId><fullGameAccess>maybe</fullGameAccess></User>"},
                {profile, "<ProfileData><PercentToLevel>1e3</PercentToLevel><Rating>INF</Rating><Rep>-INF</Rep><Boost>NaN</Boost></ProfileData>"},
                {profile, "<ProfileData><PercentToLevel> 2.5 </PercentToLevel><Rating>0x1p3</Rating><Rep>1.5f</Rep><Cash>Infinity</Cash></ProfileData>"},
                {profile, "<ProfileData><PercentToLevel>abc</PercentToLevel><Level>7</Level></ProfileData>"},
                {profile, "<ProfileData><Level>3000000000</Level><IconIndex>-0</IconIndex><Score>1_000</Score></ProfileData>"},
                {badges, "<BadgeBundle><Badges><BadgeInput><BadgeDefinitionId>1</BadgeDefinitionId><SlotId>2</SlotId></BadgeInput><BadgeInput><SlotId>3</SlotId></BadgeInput></Badges></BadgeBundle>"},
                {badges, "<BadgeBundle><BadgeInput><SlotId>9</SlotId></BadgeInput></BadgeBundle>"},
                {badges, "<BadgeBundle><Badges/></BadgeBundle>"},
                {badges, "<BadgeBundle/>"},
        };
        List<Map<String, String>> out = new ArrayList<>();
        for (String[] probe : probes) {
            Map<String, String> m = new LinkedHashMap<>();
            m.put("class", probe[0]);
            m.put("input", probe[1]);
            try {
                Object v = JAXBUtility.unMarshal(probe[1], Class.forName(probe[0]));
                m.put("result", JAXBUtility.marshal(v));
            } catch (Throwable t) {
                Throwable root = t;
                while (root.getCause() != null) root = root.getCause();
                m.put("error", t.getClass().getName() + " caused by " + root.getClass().getName());
            }
            out.add(m);
        }
        return out;
    }

    private static String typeName(RuntimeNonElement target) {
        Type t = target.getType();
        return t instanceof Class ? ((Class<?>) t).getName() : t.getTypeName();
    }

    /** The default of a property in {@code new X()}, as plain JSON. */
    private static Object defaultValue(Object v) {
        if (v == null || v instanceof Number || v instanceof Boolean || v instanceof String) return v;
        if (v instanceof Collection) return ((Collection<?>) v).isEmpty() ? new ArrayList<>() : "non-empty list";
        if (v instanceof Enum) return ((Enum<?>) v).name();
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("new", v.getClass().getName());
        return m;
    }

    private static Field field(Class<?> c, String name) {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            try {
                return k.getDeclaredField(name);
            } catch (NoSuchFieldException ignored) {
            }
        }
        throw new IllegalStateException(c.getName() + " has no field " + name);
    }

    private static Object newInstance(Class<?> c) throws Exception {
        Constructor<?> k = c.getDeclaredConstructor();
        k.setAccessible(true);
        return k.newInstance();
    }

    /** A deterministic, fully populated instance, nested beans to a few levels. */
    private static Object fill(Class<?> c, int depth) throws Exception {
        Object o = newInstance(c);
        for (Class<?> k = c; k != null && k != Object.class; k = k.getSuperclass()) {
            for (Field f : k.getDeclaredFields()) {
                int mod = f.getModifiers();
                if (Modifier.isStatic(mod) || Modifier.isFinal(mod) || Modifier.isTransient(mod)
                        || f.isAnnotationPresent(XmlTransient.class)) continue;
                f.setAccessible(true);
                int seed = (c.getSimpleName() + "." + f.getName()).hashCode();
                if (Collection.class.isAssignableFrom(f.getType())) {
                    Type item = ((ParameterizedType) f.getGenericType()).getActualTypeArguments()[0];
                    List<Object> list = new ArrayList<>();
                    for (int i = 0; i < 2; i++) {
                        Object v = value((Class<?>) item, seed + i, depth);
                        if (v != null) list.add(v);
                    }
                    f.set(o, list);
                } else {
                    Object v = value(f.getType(), seed, depth);
                    if (v != null || !f.getType().isPrimitive()) f.set(o, v);
                }
            }
        }
        return o;
    }

    private static Object value(Class<?> t, int seed, int depth) throws Exception {
        int abs = Math.abs(seed % 1_000_000);
        if (t == int.class || t == Integer.class) return seed % 2000 - 1000;
        if (t == long.class || t == Long.class) return (long) seed * 1_000_003L;
        if (t == short.class || t == Short.class) return (short) (seed % 30000);
        if (t == byte.class || t == Byte.class) return (byte) seed;
        if (t == boolean.class || t == Boolean.class) return (seed & 1) == 1;
        if (t == float.class || t == Float.class) return FLOATS[abs % FLOATS.length];
        if (t == double.class || t == Double.class) return DOUBLES[abs % DOUBLES.length];
        if (t == String.class) return "s" + abs;
        if (t == XMLGregorianCalendar.class) {
            String[] forms = {"2026-01-02T03:04:05.678Z", "2026-03-04T05:06:07+10:00", "2026-05-06T07:08:09"};
            return DatatypeFactory.newInstance().newXMLGregorianCalendar(forms[abs % forms.length]);
        }
        if (t.isEnum()) {
            Object[] constants = t.getEnumConstants();
            return constants[abs % constants.length];
        }
        if (t.getName().startsWith("com.soapboxrace.")) {
            // Abstract (polymorphic) slots stay empty: which subclass goes there is the caller's business.
            return depth < 3 && !Modifier.isAbstract(t.getModifiers()) ? fill(t, depth + 1) : null;
        }
        throw new IllegalStateException("no sample value for " + t.getName());
    }

    private static void write(Path file, Object value) throws Exception {
        try (Writer w = Files.newBufferedWriter(file, StandardCharsets.UTF_8)) {
            GSON.toJson(value, w);
            w.write('\n');
        }
        System.out.println("wrote " + file);
    }
}
