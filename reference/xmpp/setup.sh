#!/bin/sh
# Run Openfire's autosetup once at build time, then undo the one step where
# autosetup differs from the web wizard SBRW's guide clicks through: autosetup
# turns on user.scramHashedPasswordOnly, the wizard leaves it off. With it on,
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
rm -rf logs/*
