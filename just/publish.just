# Publish a crate to crates.io.
[group('publish')]
publish args="":
  cargo publish --locked {{ args }}

# List files included in the packaged crate.
[group('publish')]
package_list args="":
  cargo package --list --exclude-lockfile {{ args }}

# Publish all crates in dependency order
[group('publish')]
publish_all: publish_wasm_dbms_all publish_wasi_dbms_all

# Publish all wasm-dbms crates in dependency order
[group('publish')]
publish_wasm_dbms_all: publish_wasm_dbms_macros publish_wasm_dbms_api publish_wasm_dbms_memory publish_wasm_dbms publish_wasm_dbms_sql

[group('publish')]
publish_wasm_dbms_macros: (_publish "wasm-dbms-macros")

[group('publish')]
publish_wasm_dbms_api: (_publish "wasm-dbms-api")

[group('publish')]
publish_wasm_dbms_memory: (_publish "wasm-dbms-memory")

[group('publish')]
publish_wasm_dbms: (_publish "wasm-dbms")

[group('publish')]
publish_wasm_dbms_sql: (_publish "wasm-dbms-sql")

# Publish all WASI-specific crates in dependency order
[group('publish')]
publish_wasi_dbms_all: publish_wasi_dbms_memory publish_wasi_dbms_key_value_memory

[group('publish')]
publish_wasi_dbms_memory: (_publish "wasi-dbms-memory")

[group('publish')]
publish_wasi_dbms_key_value_memory: (_publish "wasi-dbms-key-value-memory")

# Publish a crate with retry logic to handle registry lag
[private]
_publish crate:
  #!/usr/bin/env bash
  set -euo pipefail
  max_retries=5
  delay=15
  for attempt in $(seq 1 $max_retries); do
    echo "Publishing {{crate}} (attempt $attempt/$max_retries)..."
    if cargo publish -p {{crate}}; then
      echo "Successfully published {{crate}}"
      exit 0
    fi
    if [ "$attempt" -lt "$max_retries" ]; then
      echo "Publish failed, retrying in ${delay}s..."
      sleep "$delay"
    fi
  done
  echo "Failed to publish {{crate}} after $max_retries attempts"
  exit 1
