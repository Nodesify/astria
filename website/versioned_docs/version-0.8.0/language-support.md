---
sidebar_position: 7
title: Language support
---

# Language support

Extraction covers **21 languages** via tree-sitter grammars:

Python, JavaScript, TypeScript, Rust, Go, Java, C, C++, Ruby, Swift, Kotlin, Scala, PHP, C#, Lua, Haskell, Elixir, Bash, Dart, Zig, CSS.

Each language has its own config module in `crates/graphify-extract/src/langs/`. Every module provides a `LanguageConfig` specifying which AST nodes represent classes, functions, and relationships, plus docstring capture rules.

## Adding a new language

Adding a new language means:

1. Adding a new config file in `crates/graphify-extract/src/langs/`
2. Registering it in `langs/mod.rs`

The extraction pass is uniform across languages — only the config differs — so a new language starts producing nodes and edges as soon as its grammar rules are mapped.
