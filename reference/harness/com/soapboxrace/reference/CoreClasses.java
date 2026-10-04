package com.soapboxrace.reference;

import java.io.IOException;
import java.net.URISyntaxException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;
import java.util.stream.Collectors;
import java.util.stream.Stream;

/** Every top-level class compiled from the core, found by walking the classes directory. */
final class CoreClasses {
    static final String UPSTREAM = "SoapboxRaceWorld/soapbox-race-core@d3edcd002514eb5265ad44484d79c4d2541ab055";

    private CoreClasses() {
    }

    static List<Class<?>> load() throws IOException, URISyntaxException, ClassNotFoundException {
        Path root = Paths.get(CoreClasses.class.getProtectionDomain().getCodeSource().getLocation().toURI());
        Path core = root.resolve("com/soapboxrace/core");
        List<String> names;
        try (Stream<Path> s = Files.walk(core)) {
            names = s.map(p -> root.relativize(p).toString())
                    .filter(n -> n.endsWith(".class") && !n.contains("$"))
                    .map(n -> n.substring(0, n.length() - ".class".length()).replace('/', '.'))
                    .sorted()
                    .collect(Collectors.toList());
        }
        List<Class<?>> classes = new ArrayList<>();
        for (String n : names) classes.add(Class.forName(n));
        return classes;
    }
}
