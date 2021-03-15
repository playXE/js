**This branch is not developed, check out rewrite branch for updates**

# starlight

Starlight is a JS engine in Rust which focuses on performance rather than ensuring 100% safety of JS runtime.


# Features
- Bytecode interpreter
- GC
- Polymorphic inline caches for objects and variable lookups


# Why?

I was developing my own JS-like language but then I realized that there's no point in it and decided to try to create optimized JS engine with JIT,inline caches, fast GC and other fun stuff that is interesting me.


# Starlight VS Boa
Starlight is much faster than Boa in a lot of ways one of which is object property accesses but Starlight is designed more like JS engine that can be potentionally used in web browsers like V8 and JSC and not something small and embedabble (we have 1.6MB binary through!).

# TODO
- Complete support for full ES5.1 and some part of ES6 (we already support spread,const and let).
- Advanced conservative on stack garbage collector (no more reference counted roots yay!)
- JIT compiler
- Benchmarks against SpiderMonkey without JIT
