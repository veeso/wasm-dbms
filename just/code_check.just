alias lint := clippy

# Format Markdown, Rust, TOML, and YAML files.
[group('code-check')]
fmt *args="":
  dprint fmt {{ args }}

# Check formatting without changing files.
[group('code-check')]
fmt_check *args="":
  dprint check {{ args }}

# Run Clippy for every workspace package.
[group('code-check')]
clippy args="":
  cargo clippy --workspace --all-features {{ args }}

# Inspect the resolved dependency graph.
[group('code-check')]
dependency_tree args="":
  cargo tree --workspace --all-features {{ args }}

# Build Rust documentation and deny warnings.
[group('code-check')]
doc args="":
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps {{ args }}

# Check advisories, licenses, bans, and dependency sources.
[group('code-check')]
deny args="":
  cargo deny check {{ args }}

# Audit GitHub Actions workflows.
[group('code-check')]
zizmor args="":
  zizmor .github/workflows {{ args }}

# Align Markdown tables in place.
[group('code-check')]
fmt_md_tables file:
  fmt-md-tables -i {{ quote(file) }}

# Scan paths for verified or unknown secrets.
[group('code-check')]
scan_secrets *args=".":
  trufflehog filesystem {{ args }} --results=verified,unknown --fail --fail-on-scan-errors --force-skip-binaries --no-update

# Run the complete local quality gate.
[group('code-check')]
check:
  just fmt_check
  just clippy "-- -D warnings"
  just doc
  just deny
  just test
