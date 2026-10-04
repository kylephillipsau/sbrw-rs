# The reference rig

sbrw-rs is a 1:1 port of [soapbox-race-core](https://github.com/SoapboxRaceWorld/soapbox-race-core).
Its tests come from running the original Java, not from reading it. This
directory builds and runs that original code so it can record fixtures.

Nothing here ships. The recordings are committed, so building and testing
sbrw-rs needs neither Java nor this rig.

## Recording

```sh
./tools/fetch-upstream.sh                            # once
./reference/record.sh                                # every scenarios/**/*.json -> fixtures/recorded/
./reference/record.sh scenarios/user/register_and_login.json
./reference/record.sh vectors                        # JDK vectors -> fixtures/vectors/
./reference/record.sh dtos                           # XML classes -> fixtures/dto/
```

`dtos` writes three files:
- `schema.json`: JAXB's runtime model of every XML class, including properties in marshalling order, element and attribute names, types, wrappers and `new X()` defaults. `tools/gen.py dtos` generates `crates/sbrw-xml/src/dto/` from it.
- `golden.json`: what `JAXBUtility.marshal` writes for a fully populated instance and for `new X()` of each class. The generated Rust must write the same bytes.
- `unmarshal.json`: how `JAXBUtility.unMarshal` reads odd input, such as bad numbers, unknown elements, namespaces and repeats.

It needs Docker and nothing else. The first run downloads MySQL 8 and about
150 MB of Maven dependencies, which are cached in the `sbrw-m2` volume.

## How it works

1. **`prepare.py`** copies the upstream core into `reference/build/core` (gitignored) and makes it deterministic with counted, mechanical patches:
   - every `new Random()`, `Math.random()`, `RandomUtils.nextInt` and bare `Collections.shuffle` goes through one seeded `java.util.Random`;
   - every clock read (`LocalDateTime.now()`, `new Date()`, `System.currentTimeMillis()`, ...) goes through a settable clock;
   - `UUID.randomUUID()` (session tokens) becomes a counter: `00000000-0000-4000-8000-000000000001`, `…002`, and so on.

   It fails if a patch count changes or any unpatched source of randomness or time remains.
2. **`pom.xml`** builds those sources as plain classes. Thorntail is gone; Hibernate 5.3.13 and RESTEasy 3.9.0 are the versions Thorntail 2.6 provided.
3. **The harness** (`harness/`) plays the part of the Java EE container:
   - **Injection and lifecycle:** beans and their dependencies are wired up and `@PostConstruct` runs, as the container did.
   - **Transactions:** each EJB call gets its own REQUIRED transaction. The JAX-RS resources aren't EJBs, so every business call from them commits separately.
   - **Exceptions:** `EngineException` is an application exception, so work done before it is thrown still commits. Any other exception rolls back and reaches the caller wrapped in `EJBException`.
   - **Async methods** (`@Asynchronous`) run after the current step.
   - **CDI events** are delivered to their `@Observes` methods.
   - **Timers** fire only when a scenario says so.
   - **HTTP:** the original resources, filters, marshaller and exception mapper run in-process through RESTEasy's mock dispatcher.
4. **Fakes** stand in for external services (`Fakes.java`):
   - **Openfire:** the XMPP connection and the REST client record what would have been sent, so every server push is part of the fixture.
   - **Redis:** in-memory, with Redis-on semantics. With Redis off upstream, presence and matchmaking silently don't work.
   - **Sentry** is silenced.
5. **MySQL 8** loads the original `Schema.sql` and `Data.sql`, with case-insensitive names as on the Windows setups the original targeted. Between scenarios the runtime tables are truncated and the `parameter` table restored.

## Scenarios and fixtures

A scenario (`scenarios/**/*.json`, see `Scenario.java`) contains:
- a seed and a starting time;
- optional parameter overrides;
- steps: HTTP requests, clock advances, timer firings, scheduled jobs.

The recorded fixture (`fixtures/recorded/**/*.json`) contains:
- every response (status, headers, body);
- the side effects of each step: XMPP pushes, Openfire calls, timers, Redis writes;
- a snapshot of every runtime table at the end.

sbrw-rs replays the same scenario with the same seed, clock and UUID counter, and must produce the same fixture.

The replay lives in `crates/sbrw-testkit`:

```sh
cargo test -p sbrw-testkit --test replay                                   # every scenario
SBRW_REPLAY_REPORT=1 cargo test -p sbrw-testkit --test replay -- user/     # with full reports
```

- **What it compares:** each step's status, `Content-Type` and `Connection` headers, and body. Bodies are compared exactly, except JSON, which is compared as data. It also compares every side effect, and every runtime table row by row and column by column.
- **Where its output goes:** what sbrw-rs did is written in the fixture format to `<cargo target>/tmp/replay/`, so any diff tool can compare it with the recording.
- **Stale recordings:** a recording whose steps no longer match its scenario is rejected, with a reminder to re-record.

Known differences are normalised by the replay, never by editing fixtures:
- **Java stack traces** in error bodies.
- **Stored passwords:** sbrw-rs stores `argon2id(sha1)` instead of the bare SHA-1. This normalisation lands with the user port.

Smaller differences that no recording exercises yet:
- **Number formatting:** `Float.toString`/`Double.toString` match JDK 11 exactly from 10⁻³ up to 10⁷, the range where Java prints plain decimals. Outside it, JDK 11 sometimes prints extra digits (`2.2856919E9` where the shortest form is `2.285692E9`). Both read back as the same number (`fixtures/vectors/java_number_to_string.json`).
- **Collation:** email and name lookups ignore case as MySQL's `utf8mb4_unicode_ci` does, but only for ASCII. MySQL also folds accents and ignores trailing spaces.

## Upstream quirks the rig has to work around

- **Database time:** four named queries compare against MySQL's `CURRENT_TIMESTAMP`, and two columns default to it, which the Java source patches can't reach. The harness sets MySQL's session clock to the reference clock at the start of every transaction. sbrw-rs passes its own clock into those queries.

- **`StartupBO.init`** inserts the online-stats row a second time, in the same second as `OnlineUsersBO`'s own `@PostConstruct`. Real servers survive because a cold start pushes the two into different seconds. The harness skips the duplicate; see `port/decisions.toml`.
