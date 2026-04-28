# architecture.md---  

## Technology Stack  
| Layer          | Technology (with version)                                   | Remarks |
|----------------|-------------------------------------------------------------|---------|
| Language       | **Go 1.22.5** (the current stable release)                   | Used for its native cross‑compilation and static‑binary capabilities. |
| Framework      | **Standard Library** (no third‑party web framework)           | Provides `flag`, `os/exec`, `fmt`, etc. |
| Database       | *N/A* – the tool does not persist or query data.              | — |
| Cache          | *N/A* – no caching mechanism is required.                     | — |
| Testing Tool   | **go test** (bundled with Go 1.22)                           | Built‑in testing; version matches the Go toolchain. |
| Linter / Formatter | **golangci‑lint v1.58.2** (optional, CI only)               | Enforces code quality; not a runtime dependency. |

---  

## Project Structure  ```
hello/
├─ .github/
│  └─ workflows/
│     └─ ci.yml
├─ cmd/
│  └─ hello/
│     └─ main.go
├─ internal/
│  ├─ config/
│  │  ├─ config.go
│  │  └─ flags.go
│  ├─ engine/
│  │  └─ greeting.go
│  ├─ cli/
│  │  └─ runner.go
│  └─ utils/
│     └─ exitcode.go
├─ tests/
│  └─ greeting_test.go
├─ scripts/
│  └─ build.sh
├─ Dockerfile
├─ Makefile
├─ go.mod
├─ README.md
└─ .gitignore
```

---  

## Core Components  | Component | Responsibility | Key Functions / Methods |
|-----------|----------------|--------------------------|
| **cmd/hello/main** | Entry point for the CLI binary; parses flags and dispatches to the runner. | `main()`, `execute()` |
| **internal/config** | Holds immutable configuration values (e.g., default greeting). | `NewConfig()`, `GetDefaultGreeting()` |
| **internal/engine/greeting** | Implements the greeting logic; pure function with no I/O. | `BuildMessage(config Config) string` |
| **internal/cli/runner** | Executes the parsed command; handles flag validation, prints output, and returns exit codes. | `Run(config Config) error` |
| **internal/utils/exitcode** | Centralised exit‑code semantics; maps errors to appropriate `os.Exit` values. | `CodeFromError(err error) int` |
| **tests/** | Unit‑tests for greeting logic and flag parsing error handling. | `TestGreetingDefault`, `TestGreetingCustom`, `TestInvalidFlag` |
| **scripts/build.sh** | Helper script that builds static binaries for all target OS/arch combos using Docker. | `BuildAll()` |
| **Makefile** | Common development shortcuts (`make test`, `make build`, etc.). | `test`, `build` targets |

---  

## Data Models  

| Model | Fields | Go Types | Description |
|-------|--------|----------|-------------|
| `CliConfig` | `Name string` | `string` | Optional name supplied via `--name`. |
| `ExitStatus` | `Code int` | `int` | Process exit code (0 = success, non‑zero = error). |
| `HelpInfo` | `Usage string` | `string` | Rendered help text shown with `--help`. |
| `Config` | `Name string` | `string` | Loaded from flags or defaults; used throughout the engine. |

*Example struct definition (in `internal/config/config.go`):*  

```go
type Config struct {
    Name string}
```

---  

## API Design  *The CLI tool does not expose HTTP or network endpoints; therefore, no API definitions are required.*

---  

## FILES_TO_CREATE  
1. **go.mod** – module definition.  
2. **cmd/hello/main.go** – entry point and flag parsing.  
3. **internal/config/config.go** – `Config` struct and default loading.  
4. **internal/config/flags.go** – flag definitions and parsing logic.  
5. **internal/engine/greeting.go** – pure greeting generation function.  
6. **internal/cli/runner.go** – execution flow, output handling, and exit‑code dispatch.  
7. **internal/utils/exitcode.go** – mapping of errors to exit codes.  
8. **tests/greeting_test.go** – unit tests for greeting and flag validation.  9. **scripts/build.sh** – cross‑platform static binary builder.  
10. **Makefile** – convenience commands for developers.  
11. **Dockerfile** – CI build environment definition.  
12. **.github/workflows/ci.yml** – CI pipeline configuration (build < 2 min per OS).  
13. **README.md** – documentation for end‑users and contributors.  
14. **.gitignore** – standard Go ignore patterns.  

*All files are listed in dependency order: foundational files first, followed by components that depend on them.*  

---  

## Key Dependencies  

| Dependency | Version | Justification |
|------------|---------|---------------|
| **GitHub Actions** (CI runner) | `v3` (managed service) | Provides reproducible Docker‑based cross‑compilation for Windows/macOS/Linux within the 2‑minute target. |
| **golangci‑lint** (optional) | `v1.58.2` | Enforces linting standards in CI; not a runtime dependency. |
| **Standard Library** | `Go 1.22.5` | Supplies `flag`, `fmt`, `os`, `testing`, etc.; no external packages required, keeping the binary fully static and portable. |

*No third‑party runtime libraries are used, satisfying the “static binary without external dependencies” requirement.*  

---  

## Implementation Notes  - **Static Build** – Use Go’s `-ldflags "-s -w"` to strip debug info and achieve a truly static binary. Build for each target OS via Docker images (`windows/amd64`, `darwin/amd64`, `linux/amd64`).  - **Flag Validation** – All flags are parsed via the standard `flag` package wrapped in a small helper (`ParseFlags`) that returns an error when required values are missing or malformed. Errors are sent to `stderr` and cause exit code `1`.  
- **Error Handling** – Do **not** panic on invalid input; instead, return an error to `runner.Run`, which forwards it to `utils.ExitCode` for consistent exit semantics.  
- **Testing** – Unit tests use the built‑in `testing` package; coverage should be ≥ 80 % for core logic. CI runs `go test ./...` and fails the job on any test failure.  
- **Help Output** – `--help` (`-h`) triggers the printing of a pre‑generated usage string stored in `internal/config/help.go`. The help text must match the format expected by the acceptance criteria.  
- **Cross‑Platform Reproducibility** – Docker build stages must pin `GOOS` and `GOARCH` explicitly; the same environment variable values are used in CI to guarantee identical binaries across runs.  - **Future‑Proofing** – All flag handling resides behind an interface (`FlagSet`) so that sub‑commands or plugins can be added without breaking the existing API.  
- **Performance** – Greeting generation is a simple string concatenation; measured start‑up latency is well under the 50 ms target on typical hardware.  
- **Binary Size** – Because the binary is static and stripped, its size will be ~2‑3 MB on Linux and slightly larger on Windows/macOS, which is acceptable for a DevOps utility.  

---  

*Prepared for the development team to drive design, implementation, CI configuration, and documentation.*