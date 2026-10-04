#!/usr/bin/env bash
# Inside the recorder container: compile the prepared build, then record.
#   run.sh [scenarios/x.json ...]   record scenarios (all when none are given)
#   run.sh vectors                  write fixtures/vectors/*.json
#   run.sh dtos                     write fixtures/dto/{schema,golden}.json
set -euo pipefail
cd /repo/reference/build/core
mvn -B -q compile dependency:build-classpath -Dmdep.outputFile=target/classpath.txt
classpath="target/classes:$(cat target/classpath.txt)"
if [ "${1:-}" = vectors ]; then
    exec java -Duser.timezone=UTC -cp "$classpath" com.soapboxrace.reference.Vectors /repo
fi
if [ "${1:-}" = dtos ]; then
    exec java -Duser.timezone=UTC -cp "$classpath" com.soapboxrace.reference.Dtos /repo
fi
exec java -Duser.timezone=UTC -cp "$classpath" com.soapboxrace.reference.Recorder /repo "$@"
