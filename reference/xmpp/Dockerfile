# Openfire 4.5.0, the release nearest SBRW's fork (4.5.0-SNAPSHOT of July 2019),
# with the nonSASL plugin SBRW's setup guide installs. See docs/protocol/xmpp.md.
FROM eclipse-temurin:11.0.24_8-jre-jammy
ADD --checksum=sha256:4d44ef9e4948f00b54b57c5366a83c5c1d48d48877e97c1f3d4bace6d9236db5 \
    https://github.com/igniterealtime/Openfire/releases/download/v4.5.0/openfire_4_5_0.tar.gz /tmp/openfire.tar.gz
ADD --checksum=sha256:7ef6fde7e51c3cfeaf84ee23342a555317dc99b0339c69b0fefe90e91e951636 \
    https://github.com/igniterealtime/openfire-nonSaslAuthentication-plugin/releases/download/v1.0.1/nonSaslAuthentication.jar /tmp/nonSaslAuthentication.jar
RUN tar xzf /tmp/openfire.tar.gz -C /opt && mv /tmp/nonSaslAuthentication.jar /opt/openfire/plugins/ && rm /tmp/openfire.tar.gz
COPY openfire.xml /opt/openfire/conf/openfire.xml
COPY java.security /opt/openfire/conf/java.security
COPY setup.sh /opt/openfire/setup.sh
WORKDIR /opt/openfire
RUN ./setup.sh
EXPOSE 5222
# As SBRW's sbrwopenfirelauncher.bat ran it, plus TLS 1.0 back on: JDK 11 turns it off.
CMD ["java", "-server", "-Djsse.enableCBCProtection=false", \
     "-Djava.security.properties=/opt/openfire/conf/java.security", \
     "-DopenfireHome=/opt/openfire", "-Dopenfire.lib.dir=/opt/openfire/lib", \
     "-Dlog4j.configurationFile=/opt/openfire/lib/log4j2.xml", "-jar", "/opt/openfire/lib/startup.jar"]
