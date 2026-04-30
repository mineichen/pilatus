# Testing the book
If both `cargo build` and `carglooko test` are executed, there are several build-config dependent artifacts, and the -L flag cannot handle that.

To setup the environment for mdbook, run the following to assure, there are not duplicated dependencies
```bash
cargo clean --target-dir target
cargo build --target-dir target 
```

Test the book with
```bash
mdbook test -L target/debug/deps
```
