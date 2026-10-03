import "./just/bench.just"
import "./just/build.just"
import "./just/changelog.just"
import "./just/code_check.just"
import "./just/publish.just"
import "./just/test.just"

WASM_DIR := env("WASM_DIR", "./.artifact")

# Lists all the available commands
default:
    @just --list
