# LLM providers

`pagebridge` is LLM-agnostic. Every model call goes through `LlmProvider`. v0.1
ships three providers.

| Provider           | Crate                       | Endpoint / mode             | JSON mode               |
|--------------------|-----------------------------|-----------------------------|-------------------------|
| Ollama             | `pagebridge-llm-ollama`     | HTTP `/api/chat`            | `format: "json"`        |
| OpenAI-compatible  | `pagebridge-llm-openai`     | HTTP `/v1/chat/completions` | `response_format: json` |
| Anthropic          | `pagebridge-llm-anthropic`  | HTTP `/v1/messages`         | Tool-use forcing        |
| Embedded llama.cpp | `pagebridge-llm-llamacpp`   | In-process GGUF             | GBNF grammar            |

## Ollama (local-first default)

`OllamaProvider::new(url, model)` or `OllamaProvider::local_default()` (uses
`http://localhost:11434` and `qwen2.5:7b`). Streaming is disabled in v0.1.

JSON mode passes `format: "json"` and, if the first response does not parse as
JSON, retries once with a reminder appended to the user message.

Recommended models:

- Navigation prompts: `qwen2.5:7b`, `qwen2.5:14b` (small, fast).
- Summarization: `qwen2.5:14b` or `llama3.1:8b`.
- Larger machines: `qwen2.5:32b`.

## OpenAI-compatible

Constructors:

- `openai(api_key, model)` for OpenAI proper.
- `vllm(base_url, model)` for self-hosted vLLM servers (no API key).
- `lm_studio(model)` for LM Studio at `http://localhost:1234`.
- `custom(url, key, model)` for arbitrary endpoints.

429 responses honor the `Retry-After` header. Transient retries on 5xx and
connect/timeout. Use `LlmConfig` to tune.

## Anthropic

`AnthropicProvider::new(api_key, model)`. Default model
`claude-haiku-4-5-20251001`. JSON mode is implemented via tool-use forcing:
declare a single tool whose `input_schema` is the requested JSON schema, then
set `tool_choice` to that tool. The tool call's input IS the JSON response.
This gives Anthropic models grammar-constrained outputs with no additional
parsing.

## Embedded llama.cpp

`LlamaCppProvider::from_gguf(path)` loads a GGUF model in-process. No HTTP
server, no Ollama daemon. Two compile modes:

- **default**: stub mode. The provider compiles to a portable stub that
  returns an explicit "driver not enabled" error on every call. The GBNF
  grammar lowering for JSON schemas is still available via the public
  `grammar` module so callers can inspect what would be sent to the model.
- **`llamacpp-driver` feature**: links the real `llama-cpp-2` crate.
  Requires a C++ toolchain on the build host.

Hardware acceleration is opt-in via additional cargo features, each of which
implies `llamacpp-driver`:

| Feature   | Backend                       |
|-----------|-------------------------------|
| `cuda`    | NVIDIA CUDA                   |
| `metal`   | Apple Metal                   |
| `vulkan`  | Vulkan                        |

GBNF grammar lowering covers the schema fragments pagebridge prompts use
(`object` with required/optional `properties`, `array`, `string`, `integer`,
`number`, `boolean`, `enum`, `oneOf`). Anything outside that surface falls
back to an unconstrained JSON rule. `LlamaCppProvider::supports_grammar()`
returns `true`, which the synthesizer uses to skip JSON-shape repair prompts.

GGUF download workflow:

```bash
mkdir -p ~/.pagebridge/models
curl -L -o ~/.pagebridge/models/qwen2.5-7b-q4.gguf \
  https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m.gguf
```

Then from the umbrella crate:

```rust
let bridge = pagebridge::sqlite_with_llamacpp(
    "./pagebridge.db",
    "/home/me/.pagebridge/models/qwen2.5-7b-q4.gguf",
).await?;
```

## Latency hints

Approximate end-to-end latencies on consumer hardware with a 5000-leaf corpus
and a one-question ask:

- Ollama qwen2.5:7b: ~3 to 6 s (mostly model inference).
- OpenAI gpt-4o-mini: ~1.5 to 3 s (network bound).
- Anthropic claude-haiku-4-5: ~1 to 2 s.

Navigation typically takes 1 to 3 LLM calls, synthesis 1.
