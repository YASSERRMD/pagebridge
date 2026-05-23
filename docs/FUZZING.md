# Fuzzing pagebridge

The `fuzz/` directory holds `cargo-fuzz` harnesses for the parsers and
serializers that touch external bytes. Run any harness with:

```bash
cargo install cargo-fuzz   # one-time, requires nightly
cd fuzz
cargo +nightly fuzz run node_id_parser
cargo +nightly fuzz run doc_id_parser
cargo +nightly fuzz run summary_cache_json
```

Each harness:

- `node_id_parser` -- arbitrary bytes into `NodeId::new`; asserts no panic
  and that any accepted value round-trips.
- `doc_id_parser` -- arbitrary bytes into `DocId::new`; asserts the length
  invariant on accepted values.
- `summary_cache_json` -- arbitrary bytes into `serde_json::from_slice::<SummaryCacheEntry>`
  to ensure decoding never panics.

## Property tests

Adapter-shaped property tests live next to the unit tests
(`crates/pagebridge-core/tests/proptest_adapter.rs`). They use `proptest` to
drive random sequences against `MemoryAdapter` and assert the storage
contract (`upsert/get` round-trips, `delete_document` removes every node).

```bash
cargo test --workspace --test proptest_adapter
```

## Roadmap

- Add fuzz targets for the markdown ingester and each LLM provider response
  parser (Ollama, OpenAI, Anthropic).
- Run every harness for at least 1 hour in a nightly CI schedule.
- Bring up loom tests for the embedded adapter's concurrent reader scenario.
