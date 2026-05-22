# Proxy Translation Design

The proxy should model protocol behavior, not examples. A capture can prove a reader shape, but production code should not branch on a module name, character name, area name, fixture filename, or one custom asset row.

## Rules

- Prefer decompiled client/server reader order. A parser should advance the same fields, branches, and count widths as the original code.
- Prefer typed protocol variants over named examples. `D5FF` with byte counts and `D5FF` with word counts are valid siblings; `D5FF for one area` is not.
- Prefer session state when bytes are aliases. Compact object ids, current-player ids, active object lifetimes, and resource stacks should be resolved through observed state where possible.
- Prefer resource tables over hard-coded asset rows. `baseitems.2da`, `placeables.2da`, and similar tables should come from the active module HAK order or an explicit configured source.
- Keep fixture names in tests. Tests should identify the capture that proved the rule; production code should describe the rule itself.

## Adding A New Packet Shape

1. Add or reuse a fixture that demonstrates the source and target reader behavior.
2. Identify the original Diamond and EE reader order, including bit ownership and count widths.
3. Decide whether the difference is a dialect variant, a state alias, a resource-table value, or an unsupported packet.
4. Implement the smallest typed parser or state/resource lookup that proves the full cursor before rewriting.
5. Add a regression test whose name may mention the fixture, while keeping production identifiers and string literals generic.

## Guardrail

`cargo test -p hgbridge-proxy2 production_translators_do_not_name_fixture_examples` scans production translator code for named capture/example terms. It intentionally strips comments and `#[cfg(test)]` modules first, and it skips explicit resource profiles because those are data declarations rather than packet parser branches.
