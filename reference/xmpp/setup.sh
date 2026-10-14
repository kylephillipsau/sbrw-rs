#!/bin/sh
# Run Openfire's autosetup once at build time, then make the settings SBRW's
# guide makes in the admin console. Autosetup also turns on
# user.scramHashedPasswordOnly, which the web wizard leaves off; with it on,
# the nonSASL plugin refuses every jabber:iq:auth login.
set -eu
cd /opt/openfire
java -Djava.security.properties=/opt/openfire/conf/java.security -DopenfireHome=/opt/openfire \
    -Dopenfire.lib.dir=/opt/openfire/lib -Dlog4j.configurationFile=/opt/openfire/lib/log4j2.xml \
    -jar lib/startup.jar > /tmp/setup.log 2>&1 &
pid=$!
i=0
until grep -q "Finished processing all plugins" /tmp/setup.log; do
    i=$((i + 1)); [ $i -lt 120 ] || { cat /tmp/setup.log; exit 1; }; sleep 1
done
kill $pid; wait $pid || true
grep -l "scramHashedPasswordOnly" embedded-db/* | xargs sed -i "s/'user.scramHashedPasswordOnly','true'/'user.scramHashedPasswordOnly','false'/"
! grep -q "'user.scramHashedPasswordOnly','true'" embedded-db/*
# HSQLDB replays its redo log at the next start: one session, then COMMIT.
{
    echo "/*C99*/SET SCHEMA PUBLIC"
    while read -r name value; do
        echo "DELETE FROM OFPROPERTY WHERE NAME='$name'"
        echo "INSERT INTO OFPROPERTY VALUES('$name','$value',0,NULL)"
    done <<'PROPS'
register.inband false
xmpp.client.compression.policy disabled
xmpp.server.compression.policy disabled
plugin.restapi.enabled true
plugin.restapi.httpAuth secret
plugin.restapi.secret sbrw-openfire-token
PROPS
    echo "COMMIT"
    echo "DISCONNECT"
} >> embedded-db/openfire.log
rm -rf logs/*
