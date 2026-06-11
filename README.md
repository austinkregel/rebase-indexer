# rebase-indexer

Standalone code indexer for [rebase](../rebase). It walks a project (descending
into dependency trees — `node_modules`, `vendor`, `crates`, …), chunks each
file, embeds the chunks with Ollama, and writes a per-project **LanceDB** index
that the rebase app opens as a local, offline-queryable copy.

It pins the **same `lancedb` version as the rebase app**, so the index it
produces is openable by the app with no format skew.

## Usage

```sh
# one-time: pull the embedding model
ollama pull nomic-embed-text

# build an index (writes <dir>/.rebase-index)
rebase-indexer index /path/to/project \
  --ollama http://localhost:11434 --model nomic-embed-text [--lang go,rust]

# semantic search it
rebase-indexer search --index /path/to/project/.rebase-index --query "where is auth handled?"
```

Chunking is line-window based (language-agnostic) for now; tree-sitter
(boundary-aware) chunking is a planned upgrade. Languages: go, rust, php, java,
c, cpp, python, ruby, elixir, erlang, javascript/typescript (incl. jsx/tsx),
vue, html, css, shell, json, yaml, toml, sql, markdown.

## Releases

The agent downloads the matching binary at runtime. Pushing a version tag
(`git tag v0.1.0 && git push --tags`) runs `.github/workflows/release.yml`,
which builds on each platform and publishes these assets to the GitHub Release:

| Platform | Asset |
|---|---|
| Linux x86_64 | `rebase-indexer-linux-x86_64` |
| Linux arm64 | `rebase-indexer-linux-arm64` |
| macOS x86_64 | `rebase-indexer-macos-x86_64` |
| macOS arm64 | `rebase-indexer-macos-arm64` |
| Windows x86_64 | `rebase-indexer-windows-x86_64.exe` |

They're raw binaries (chmod +x after download). `workflow_dispatch` builds the
matrix without publishing, for testing.
