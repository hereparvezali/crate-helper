/// Curated list of popular Rust crates for instant zero-latency autocomplete
pub struct PopularCrate {
    pub name: &'static str,
    pub description: &'static str,
}

pub const POPULAR_CRATES: &[PopularCrate] = &[
    PopularCrate { name: "serde", description: "A generic serialization/deserialization framework" },
    PopularCrate { name: "serde_json", description: "A JSON serialization file format" },
    PopularCrate { name: "serde_yaml", description: "YAML data format for Serde" },
    PopularCrate { name: "tokio", description: "An event-driven, non-blocking I/O platform for writing asynchronous I/O" },
    PopularCrate { name: "anyhow", description: "Flexible concrete Error type built on std::error::Error" },
    PopularCrate { name: "thiserror", description: "derive(Error) for struct and enum error types" },
    PopularCrate { name: "clap", description: "A simple to use, efficient, and full-featured Command Line Argument Parser" },
    PopularCrate { name: "reqwest", description: "higher level HTTP client library" },
    PopularCrate { name: "axum", description: "Ergonomic and modular web framework built with Tokio, Tower, and Hyper" },
    PopularCrate { name: "actix-web", description: "Actix web is a powerful, pragmatic, and extremely fast web framework" },
    PopularCrate { name: "rand", description: "Random number generators and other randomness functionality" },
    PopularCrate { name: "syn", description: "Parser for Rust source code" },
    PopularCrate { name: "quote", description: "Quasi-quoting macro quote!(...)" },
    PopularCrate { name: "proc-macro2", description: "A substitute implementation of the compiler's proc_macro API" },
    PopularCrate { name: "regex", description: "An implementation of regular expressions for Rust" },
    PopularCrate { name: "chrono", description: "Date and time library for Rust" },
    PopularCrate { name: "log", description: "A lightweight logging facade for Rust" },
    PopularCrate { name: "tracing", description: "Application-level tracing and diagnostic instrumentation" },
    PopularCrate { name: "tracing-subscriber", description: "Utilities for implementing and composing tracing subscribers" },
    PopularCrate { name: "env_logger", description: "A logging implementation for log which is configured via an environment variable" },
    PopularCrate { name: "bytes", description: "Types and traits for working with bytes" },
    PopularCrate { name: "futures", description: "An implementation of futures and streams" },
    PopularCrate { name: "itertools", description: "Extra iterator adaptors, functions and macros" },
    PopularCrate { name: "parking_lot", description: "More compact and efficient implementations of standard synchronization primitives" },
    PopularCrate { name: "rayon", description: "Simple work-stealing parallelism for Rust" },
    PopularCrate { name: "async-trait", description: "Type erasure for async trait methods" },
    PopularCrate { name: "tempfile", description: "A library for managing temporary files and directories" },
    PopularCrate { name: "uuid", description: "A library to generate and parse UUIDs" },
    PopularCrate { name: "base64", description: "encodes and decodes base64 as bytes or utf8" },
    PopularCrate { name: "sha2", description: "Pure Rust implementation of the SHA-2 cryptographic hash function" },
    PopularCrate { name: "hyper", description: "A fast and correct HTTP implementation" },
    PopularCrate { name: "tower", description: "Tower is a library of modular and reusable components for building robust network clients and servers" },
    PopularCrate { name: "tower-http", description: "Tower services and middleware that are specific to HTTP" },
    PopularCrate { name: "sqlx", description: "The rust SQL toolkit. Async, pure Rust, compile-time checked queries without a DSL" },
    PopularCrate { name: "diesel", description: "A safe, extensible ORM and Query Builder for Rust" },
    PopularCrate { name: "dotenvy", description: "A dotenv implementation for Rust" },
    PopularCrate { name: "config", description: "Layered configuration system for Rust applications" },
    PopularCrate { name: "crossbeam", description: "Tools for concurrent programming" },
    PopularCrate { name: "lazy_static", description: "A macro for declaring lazily evaluated statics in Rust" },
    PopularCrate { name: "once_cell", description: "Single assignment cells and once initialization primitives" },
    PopularCrate { name: "bitflags", description: "A macro to generate structures which behave like bitflags" },
    PopularCrate { name: "smallvec", description: "Store up to a small number of items on the stack" },
    PopularCrate { name: "semver", description: "Parser and evaluator for Cargo's flavor of Semantic Versioning" },
    PopularCrate { name: "toml", description: "A native Rust encoder and decoder of TOML-formatted files and streams" },
    PopularCrate { name: "toml_edit", description: "A format-preserving TOML parser and editor" },
    PopularCrate { name: "walkdir", description: "Recursively walk a directory" },
    PopularCrate { name: "glob", description: "Support for matching file paths against Unix shell style patterns" },
    PopularCrate { name: "flate2", description: "DEFLATE compression and decompression exposed as Read/Write streams" },
    PopularCrate { name: "tar", description: "A library for reading and writing TAR archives and executing all TAR commands" },
    PopularCrate { name: "zip", description: "Library for reading and writing zip archives" },
    PopularCrate { name: "csv", description: "Fast CSV parsing with support for serde" },
    PopularCrate { name: "image", description: "Imaging library written in Rust" },
    PopularCrate { name: "indicatif", description: "A command line progress reporting library" },
    PopularCrate { name: "crossterm", description: "A crossplatform terminal library for manipulating terminals" },
    PopularCrate { name: "ratatui", description: "A library that's all about cooking up great Terminal User Interfaces" },
    PopularCrate { name: "num_cpus", description: "Get the number of CPUs on a machine" },
    PopularCrate { name: "heck", description: "Case conversion library for Rust" },
    PopularCrate { name: "url", description: "URL parser for Rust" },
    PopularCrate { name: "dashmap", description: "Blazing fast concurrent HashMap for Rust" },
    PopularCrate { name: "indexmap", description: "A hash table with consistent order and fast iteration" },
    PopularCrate { name: "hashbrown", description: "A Rust port of Google's SwissTable hash map" },
    PopularCrate { name: "pin-project-lite", description: "A lightweight version of pin-project written with declarative macros" },
    PopularCrate { name: "tower-lsp", description: "Language Server Protocol framework based on Tower" },
    PopularCrate { name: "criterion", description: "Statistics-driven micro-benchmarking library" },
    PopularCrate { name: "proptest", description: "Hypothesis-like property-based testing and shrinking" },
];

pub fn search_popular_crates(query: &str) -> Vec<&'static PopularCrate> {
    let q = query.to_lowercase();
    let mut exact = Vec::new();
    let mut starts_with = Vec::new();
    let mut contains = Vec::new();

    for c in POPULAR_CRATES {
        let name_lower = c.name.to_lowercase();
        if name_lower == q {
            exact.push(c);
        } else if name_lower.starts_with(&q) {
            starts_with.push(c);
        } else if name_lower.contains(&q) || c.description.to_lowercase().contains(&q) {
            contains.push(c);
        }
    }

    let mut result = exact;
    result.extend(starts_with);
    result.extend(contains);
    result
}
