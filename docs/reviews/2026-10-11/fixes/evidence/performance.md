# Local mailbox polling probe

Same optimized store-list workload and existing synthetic cache: 10,014 messages, with 10,000 8KB bodies; three one-row Inbox queries. The fixture contains no real mail.

| Measurement | Original audit | After fixes |
|---|---:|---:|
| Three queries after warm-up | 188ms | 9ms |
| Peak resident memory | approximately 107MiB | 11,304,960 bytes (10.8MiB) |

After-fix command:

```sh
rtk proxy /usr/bin/time -l /tmp/alice-audit-target/release/poll_bench /tmp/alice-audit-20261011/fixtures/mail.sqlite3
```

The temporary probe depends on this repository's alice-core and asserts a total of at least 10,000. Results were measured during this fix run. Timing excludes first-time schema/index setup and the warm-up query. Resident memory includes process startup. This measures local store queries, not provider synchronization or HTTP/UI end-to-end latency. The earlier debug-profile timing is deliberately excluded from the comparison.

Routine polling now reads SQL metadata/counts and deserializes only the selected page. Unicode body search streams one body at a time and retains only the requested page; it still scans scoped bodies for accurate totals. That search-latency limit is documented in the source.
