# latest-maven-version

Check Maven Central for the latest version(s) of some maven coordinates.

## Building

### Prerequisites

This tool is build with Rust so you need to have a rust toolchain and cargo installed.
If you don't, please visit [https://rustup.rs/](https://rustup.rs/) and follow their instructions.

### Building

The preferred way is to run:

```rust
make install
```
If you do not have a fairly recent make (on macOS, homebrew can install a newer version),
or don't want to use make, you can also run `cargo install --path .`.

## Usage

Run `latest-maven-version --help` for an overview of all available options.

The main usage is by providing maven coordinates in the form of `groupId:artifact`, followed by multiple `:version` qualifiers.
These version qualifier are [Semantic Version Ranges](https://www.npmjs.com/package/semver#advanced-range-syntax).
For each of the provided versions, the latest available version on maven central is printed.

Pre-releases can be included with the `--include-pre-releases` flag (or `-i` for short).

The versions are matched in order and a single version can only be matched by one qualifier.
Previous matches will – depending on the range – consume all versions that would have also been matched by later qualifiers.
Try to define the qualifiers in the order from most restrictive to least.

## Examples

Matching against minor-compatible releases

    $ latest-maven-version org.neo4j.gds:proc:~1.1:~1.3:1
    Latest version(s) for org.neo4j.gds:proc:
    Latest version matching ~1.1: 1.1.4
    Latest version matching ~1.3: 1.3.1
    Latest version matching ^1: 1.2.3


Matching against major compatible releases. Note that `1.3` does not produce any match, as it is already covered by `1.1`.

    $ latest-maven-version org.neo4j.gds:proc:1.1:1.3:1
    Latest version(s) for org.neo4j.gds:proc:
    Latest version matching ^1.1: 1.3.1
    No version matching ^1.3
    Latest version matching ^1: 1.0.0

Inclusion of pre releases.

    $ latest-maven-version org.neo4j.gds:proc:~1.1:~1.3:1 --include-pre-releases
    Latest version(s) for org.neo4j.gds:proc:
    Latest version matching ~1.1: 1.1.4
    Latest version matching ~1.3: 1.3.1
    Latest version matching ^1: 1.4.0-alpha02



License: MIT OR Apache-2.0
