# architecture.md

---

## Technology Stack| Layer            | Technology / Version                                   | Remarks |
|------------------|--------------------------------------------------------|---------|
| **Language**     | Go **1.22.2**                                          | Official stable release, full toolchain support for cross‑compilation. |
| **Framework**    | Standard Library (part of Go 1.22.2)                    | No external web framework – only `net/http` is unused; all functionality relies on the std‑lib. |
| **Database**     | *None* (no persistence required)                      | The CLI stores no data; only transient structs are used. |
| **Cache**        | *None*                                                 | No caching mechanism needed for this MVP. |
| **Testing Tool**| `go test` (Go 1.22.2) – assertions via **testify v1.8.4** | `testing` package for unit tests; `testify` provides human‑readable assertions and simplifies coverage reporting. |
| **CI/CD**        | GitHub Actions (workflow file `.github/workflows/ci.yml`) | Executes `go test`, `go vet`, and builds for multiple platforms using build tags. |
| **Build Tags**   | `// +build linux` / `// +build windows` etc.          | Enable compilation for `linux/amd64`, `darwin/amd64`, `windows/amd64` without source changes. |

---

## Project Structure```
hello-cli/
├── .github/
│   └── workflows/
│       └── ci.yml                     # CI pipeline definition
├── cmd/
│   └── hello/
│       └── main.go                    # Entry point; parses flags & prints output
├── internal/
│   └── config/
│       └── config.go                  # Holds application configuration structs and loader
├── pkg/
│   └── cli/
│       └── cli.go                     # Core CLI logic (greeting generation, flag handling)
├── test/
│   └── cli_test.go                    # Unit tests (cover flag parsing & output validation)
├── Makefile                           # Convenience commands (build, test, lint, etc.)
├── go.mod
├── go.sum
├── README.md                          # Project overview & usage instructions
└── architecture.md                    # This architecture description
```

---

## Core Components
| Component               | Responsibility                                                               | Key Functions / Methods                                      |
|-------------------------|------------------------------------------------------------------------------|---------------------------------------------------------------|
| **CLI Parser**          | Interprets command‑line flags (`-name`, `-h`, `--help`) and populates config. | `parseFlags() (*Config, error)`                               |
| **Greeting Generator** | Produces the output string based on the parsed configuration.                 | `BuildMessage(cfg *Config) string`                           |
| **Config Manager**      | Holds immutable configuration values (name, help flag, OS/arch detections).   | `NewConfig(name string, showHelp bool)`                       |
| **Output Executor**     | Writes the final message to stdout and returns an exit code.                  | `Execute(msg string) (int)` (calls `os.Stdout.WriteString`) |
| **CI Builder**          | Orchestrates cross‑platform builds and test verification in CI.               | `BuildForTarget(tag string) error` (uses `go build -tags`)    |
| **Test Suite**          | Validates behavior of parser and message builder across scenarios.            | `TestParseFlags`, `TestMessageGeneration` (in `cli_test.go`) |

---

## Data Models
### `CliConfig`
```go
type CliConfig struct {
    Name      string // Optional; value of -name/--name flag (empty means default)
    ShowHelp  bool   // True when -h/--help was supplied    TargetOS  string // Detected OS (e.g., "linux", "darwin", "windows")
    TargetArn string // Detected architecture (e.g., "amd64")
}
```

### `CliResult`
```gotype CliResult struct {
    Message string // The final string to print (e.g., "hello world\n" or "Hello, Alice!\n")
    ExitCode int   // Should be 0 on success; non‑zero on error/usage failure
}
```

*Both structs are defined in `internal/config/config.go` and exported for unit‑testing.*

---

## API Design
*The utility does **not** expose any HTTP/Network API.*

| Method | Path | Description | Request Body | Response Format |
|--------|------|-------------|--------------|-----------------|
| — | — | No external endpoints. All interaction is via CLI flags and stdout. | — | — |

**Help Output (example)**
```
Usage:
  hello [flags]

Flags:
  -h, --help   Show this help message
  -name string
               Custom name for greeting (default "world")
```

---

## FILES_TO_CREATE
1. **go.mod** – module definition (`module github.com/yourname/hello-cli`).  
2. **cmd/hello/main.go** – program entry point; loads config, calls core components, and exits with proper status.  
3. **internal/config/config.go** – defines `CliConfig`, loads parsed flags, exposes helper `NewConfig`.  
4. **pkg/cli/cli.go** – implements greeting generation and execution logic (`BuildMessage`, `Execute`).  
5. **pkg/cli/cli_test.go** – unit tests for flag parsing and message building.  
6. **test/cli_test.go** – integration‑style test verifying output on each target (uses `testing.T` and `bytes.Buffer`).  7. **Makefile** – convenience shortcuts (`make build`, `make test`, `make lint`, `make ci`).  
8. **.github/workflows/ci.yml** – GitHub Actions workflow: lint, test, coverage, and multi‑platform builds.  
9. **README.md** – usage documentation, build instructions, and contribution guide.  
10. **architecture.md** – this file (present from the start).  

*The order respects dependencies: `go.mod` → source files → tests → CI configuration.*

---

## Key Dependencies
| Dependency                              | Version      | Justification |
|----------------------------------------|--------------|----------------|
| `github.com/stretchr/testify`           | `v1.8.4`     | Provides expressive assertions (`require.Equal`, `assert.NoError`) that simplify test code and improve readability. |
| `golang.org/x/tools/go/locksmith` *(optional for lockfile hygiene)* | `v0.5.0` | Used only if lockfile (`go.sum`) needs regeneration; not a hard requirement for MVP. |
| `github.com/golangci/golangci-lint`   | `v1.58.2`    | Linter used in CI to enforce code‑style and static‑check rules; ensures code quality across contributors. |
| `github.com/google/uuid` *(optional)*   | `v1.5.0`     | Demonstrates a dependency that could be added later for UUID generation; currently not used but listed for future extensibility. |

*All other code relies exclusively on the Go standard library, guaranteeing zero external runtime dependencies.*

---

## Implementation Notes
- **Flag Parsing** – Use the standard `flag` package; however, validate `-name` value *before* passing it to `os.Stdout.WriteString` to guarantee that an empty or malformed name triggers a usage error and exit code 1.  - **Exit Codes** – Always return `0` on successful execution (including help) and `1` for any user‑error (unknown flag, missing argument). The `main` function must defer `os.Exit(code)`.  
- **Cross‑Platform Build Tags** – Define build constraints at the top of source files, e.g. `// +build windows` and `// +build !windows`. CI workflow builds with `GOOS`/`GOARCH` environment variables to produce binaries for `linux/amd64`, `darwin/amd64`, and `windows/amd64`.  
- **Testing & Coverage** – Run `go test ./... -coverprofile=coverage.out` and enforce a minimum of **80 %** total coverage via `go tool cover -func=coverage.out`. The CI step fails if the threshold is not met.  
- **Concurrency** – The binary is single‑threaded; however, design should keep the core logic pure (no global state) so that it can be safely invoked concurrently (e.g., in CI load tests).  
- **Security** – No network I/O; sanitize the `-name` value by rejecting control characters and excessive length (> 64 bytes) to prevent terminal escape‑sequence injection.  
- **Modularity** – `cmd/hello/main.go` only orchestrates; all business logic resides in `pkg/cli`. This separation enables future expansion (sub‑commands, plugins) without breaking existing behavior.  
- **Makefile Targets** –  
  - `make build` → builds default binary for the host OS.  
  - `make cross` → iterates over required `GOOS`/`GOARCH` pairs and outputs binaries under `dist/`.    - `make test` → runs unit tests with coverage report.  
  - `make lint` → runs `golangci-lint run`.  
- **CI Workflow** – The GitHub Actions file must:    1. Checkout code.  
  2. Set up Go 1.22.2.  
  3. Cache the module download directory.  
  4. Run `go vet`, `golangci-lint`, and `go test` with coverage enforcement.  
  5. Build three platform targets using the appropriate build tags and place artifacts in `dist/`.  
  6. Verify the binary’s stdout matches the expected string (`hello world` or `Hello, <name>!`).  

Following this architecture will produce a clean, test‑driven, cross‑platform CLI starter that meets all functional and non‑functional requirements outlined in the specifications.