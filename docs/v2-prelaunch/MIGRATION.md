# Migration Guide: from other RAG libraries to pagebridge 2.0

## From LangChain

```bash
pagebridge migrate from langchain --in ./langchain_store --out ./pb.db
```

Maps:

- LangChain `Document` -> pagebridge `IngestParams { SourceKind::Markdown }`.
- LangChain metadata -> `IngestParams.user_metadata`.
- LangChain vector store -> dropped. Pagebridge does not use vectors.
- LangChain retriever chains -> pagebridge `ask` calls; the chain
  composition becomes facade options.

## From LlamaIndex

```bash
pagebridge migrate from llamaindex --in ./storage --out ./pb.db
```

Maps:

- LlamaIndex `Document` -> `IngestParams`.
- LlamaIndex `IndexStore` + `DocstoreEntry` -> NodeRecord tree.
- LlamaIndex `vector_store.json` -> dropped.

## From PageIndex Cloud

```bash
pagebridge migrate from pageindex --url <url> --token <token> --out ./pb.db
```

Maps:

- PageIndex tree -> pagebridge node tree (1:1).
- PageIndex summaries -> NodeSummary.
- PageIndex retrieval API -> pagebridge `ask`.

## What does NOT migrate

- Trained custom retrieval models. Pagebridge uses BM25 + tree
  navigation; bring-your-own-embeddings is intentionally out of
  scope.
- Custom Python chains. Rewrite as pagebridge facade calls.
