---
sidebar_position: 4
title: Language support
description: The 21 languages nodesify-graphify extracts via tree-sitter, and how to add a new one with a LanguageConfig module.
keywords: [languages, tree-sitter, python, rust, typescript, go, java]
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
