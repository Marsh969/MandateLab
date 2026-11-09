# Sandbox open-file-limit shim (constrained containers only)

`near-sandbox` (neard) refuses to start unless it can raise `RLIMIT_NOFILE` to 65535:

```
Error: couldn't set the file descriptor limit to (65535, 65535), current limit = (20000, 20000)
```

Some cloud/CI containers cap the hard limit lower and forbid raising it. This shim clamps
neard's request to the existing hard limit instead of failing. It changes nothing else, is only
injected into the sandbox process, and is **not needed on a normal developer machine**.

```bash
gcc -shared -fPIC -O2 -o scripts/sandbox/build/libnofile_clamp.so scripts/sandbox/nofile_clamp.c -ldl
```

```bash
cd contracts/mandate-vault
export NEAR_SANDBOX_REAL_BIN=$(realpath $(find target/debug/build -path "*near-sandbox*" -name near-sandbox -type f | head -1))
export NEAR_SANDBOX_BIN_PATH=$(realpath ../../scripts/sandbox/near-sandbox-wrapper.sh)
cargo test --test sandbox -- --test-threads=2
```

(`NEAR_SANDBOX_REAL_BIN` exists after a first `cargo test --test sandbox` attempt has downloaded
the sandbox binary.)
