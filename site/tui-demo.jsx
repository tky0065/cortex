/* global React */
const { useState, useEffect, useRef } = React;

/* ============================================================
   TUI Demo — animated replica of the cortex CLI
   Loops through a fake `dev` workflow, showing agents progressing
   and logs streaming in real time.
   ============================================================ */

const AGENTS = ["CEO", "PM", "TECH_LEAD", "DEVELOPER", "QA", "DEVOPS"];

// Each phase mutates agent statuses + appends log lines
const SCRIPT = [
  { t: 0, log: ["08:47:10  cortex ready — type /help for commands."] },
  {
    t: 800,
    cmd: '/run dev "build a tiny CLI that prints a friendly greeting in Go"',
    log: ["08:47:12  > /run dev \"build a tiny CLI that prints a friendly greeting in Go\""],
  },
  {
    t: 1600,
    log: ["08:47:14  workflow 'dev' started (6 agents)"],
    set: { CEO: "running" },
  },
  {
    t: 2200,
    panels: { ceo: "Analyse du besoin et cadrage MVP\n\nOverview\nA minimal Go application that prints \"Hello, world!\" (or a customizable greeting) to the console. The project..." },
    log: [
      "08:47:14  ceo       | started",
      "08:47:18  ceo       |   Overview\\nA minimal Go application that prints \"Hello, world!\" (or a customizable greeting) to the console.",
    ],
  },
  {
    t: 2900,
    set: { CEO: "done", PM: "running" },
    log: [
      "08:47:23  ceo       | ✓ done",
      "08:47:23  ✦ [phase:specs-ready] complete",
      "08:47:23  pm        | started",
    ],
  },
  {
    t: 3700,
    panels: { pm: "Rédaction de specs.md\n\nspecs.md\nA minimal Go application that prints a greeting to the console. The program starts as a static binary that out..." },
    log: [
      "08:47:31  pm        |   specs.md\\nA minimal Go application that prints a greeting to the console.",
    ],
  },
  {
    t: 4400,
    set: { PM: "done", TECH_LEAD: "running" },
    log: [
      "08:47:38  pm        | ✓ done",
      "08:47:38  ✦ [phase:specs-ready] complete",
      "08:47:38  tech_lead | started",
    ],
  },
  {
    t: 5200,
    panels: { tech_lead: "Génération de architecture.md\n\nTechnology Stack**\nLanguage:** Go 1.22 (pinned in 'go.mod')\nFramework:** None (standard library only)" },
    log: [
      "08:48:13  tech_lead |   Technology Stack | Language:** Go 1.22 (pinned in `go.mod`)",
    ],
  },
  {
    t: 5900,
    set: { TECH_LEAD: "done", DEVELOPER: "running" },
    log: [
      "08:48:13  tech_lead | ✓ done",
      "08:48:13  ✦ [phase:architecture-ready] complete",
      "08:48:13  developer:README.md  | started",
      "08:48:13  developer:main.go    | started",
      "08:48:13  developer:math/rand  | started",
    ],
  },
  {
    t: 6700,
    panels: {
      readme: "Implémentation de README.md\n\nhello\nA minimal cross-platform Go command-line utility that prints a customizable greeting or runs in an optional lo...\n\nFeatures",
      main: "Generating...\n\npackage main\nimport (\n  \"flag\"\n)",
      math: "✓ DONE Generating...\n\nimport \"math/rand\"\n// deterministic seeded source (fixed seed for reproducible output)",
    },
    log: [
      "08:48:18  developer:main.go    | package main import (\"flag\")",
      "08:48:21  developer:README.md  | helloA minimal cross-platform Go command-line utility that prints a customizable greeting or runs in an optional lo...Features",
      "08:48:22  developer:main.go    | ✓ done",
      "08:48:22  developer:go.mod     | package main import (\"flag\")",
    ],
  },
  {
    t: 7500,
    set: { DEVELOPER: "done", QA: "running" },
    panels: {
      readme: "✓ DONE Implémentation de README.md\n\nhello\nA minimal cross-platform Go command-line utility that prints a customizable greeting or runs in an optional lo...\n\nFeatures",
      main: "Generating...\n\npackage main\nimport (\n  \"flag\"\n)",
      math: "✓ DONE Generating...\n\nimport \"math/rand\"\n// deterministic seeded source (fixed seed for reproducible output)",
    },
    log: [
      "08:48:31  ✦ [phase:development-done] complete",
      "08:48:31  qa        | started",
      "08:48:41  qa        |   PASS:**  - Project structure (go.mod, README.md, main.go) matches the specified layout. All required standard-library imports (fmt, os, flag, math/rand, time) are present. `parseFlags` correctly declares the `-greeting` (string) and `-loop` (bool) flags.",
    ],
  },
  {
    t: 8400,
    set: { QA: "done", DEVOPS: "running" },
    log: [
      "08:48:50  qa        | ✓ done",
      "08:48:50  developer:fix:go.mod | started",
      "08:48:53  devops    | started",
    ],
  },
  {
    t: 9200,
    set: { DEVOPS: "done" },
    log: [
      "08:49:08  devops    | ✓ wrote Dockerfile, docker-compose.yml",
      "08:49:08  ✦ [phase:complete] ./",
    ],
  },
  { t: 11500, restart: true },
];

const STATUS_GLYPH = {
  idle: "◌",
  running: "●",
  done: "✓",
  error: "✗",
};

function PipelineBar({ statuses }) {
  return (
    <div className="tui-pipeline">
      <span className="tui-pipeline__label">Pipeline</span>
      {AGENTS.map((a, i) => {
        const status = statuses[a] || "idle";
        return (
          <React.Fragment key={a}>
            <span className={`tui-pipe-item tui-pipe-item--${status}`}>
              <span className="tui-pipe-glyph">{STATUS_GLYPH[status]}</span>
              {a}
            </span>
            {i < AGENTS.length - 1 && <span className="tui-pipe-arrow">→</span>}
          </React.Fragment>
        );
      })}
    </div>
  );
}

function AgentPanel({ id, title, body, status, color }) {
  const isComplete = status === "done";
  const isRunning = status === "running";
  return (
    <div className={`tui-agent tui-agent--${status}`}>
      <div className="tui-agent__head">
        <span className="tui-agent__title" style={{ color }}>{title}</span>
        {isComplete && <span className="tui-agent__badge">✓ DONE</span>}
        {isRunning && <span className="tui-agent__badge tui-agent__badge--run">● Generating…</span>}
      </div>
      <div className="tui-agent__body">{body || ""}</div>
      <div className={`tui-agent__bar ${isComplete ? "is-complete" : ""} ${isRunning ? "is-running" : ""}`}>
        {isComplete && <span className="tui-agent__bar-label">COMPLETE</span>}
      </div>
    </div>
  );
}

function LogPane({ lines }) {
  const ref = useRef(null);
  useEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [lines.length]);
  return (
    <div className="tui-logs" ref={ref}>
      {lines.map((line, i) => {
        const isPhase = line.includes("[phase:");
        const isStarted = line.includes("| started");
        const isDone = line.includes("✓ done");
        let cls = "tui-log";
        if (isPhase) cls += " tui-log--phase";
        else if (isDone) cls += " tui-log--done";
        else if (isStarted) cls += " tui-log--started";
        return <div key={i} className={cls}>{line}</div>;
      })}
    </div>
  );
}

function CommandBar({ command, blink }) {
  return (
    <div className="tui-cmd">
      <span className="tui-cmd__prompt">{">"}</span>
      <span className="tui-cmd__text">{command}</span>
      <span className={`tui-cmd__caret ${blink ? "blink" : ""}`}>▌</span>
    </div>
  );
}

function StatusBar({ time }) {
  return (
    <div className="tui-status">
      <span className="tui-status__brand">CORTEX v0.1.6 beta</span>
      <span className="tui-status__sep">│</span>
      <span><span className="muted">PROVIDER:</span> OPENROUTER</span>
      <span className="tui-status__sep">│</span>
      <span><span className="muted">MODEL:</span> NEMOTRON-3-SUPER-120B</span>
      <span className="tui-status__sep">│</span>
      <span><span className="muted">TIME:</span> {time}</span>
      <span className="tui-status__hint">Ctrl+C or /quit to exit</span>
    </div>
  );
}

function TUIDemo({ speed = 1 }) {
  const [step, setStep] = useState(0);
  const [statuses, setStatuses] = useState({});
  const [logs, setLogs] = useState([]);
  const [panels, setPanels] = useState({
    ceo: "", pm: "", tech_lead: "",
    readme: "", main: "", math: "",
  });
  const [command, setCommand] = useState("");
  const [blink, setBlink] = useState(true);
  const [time, setTime] = useState("00:01");

  // Time counter
  useEffect(() => {
    const start = Date.now();
    const tk = setInterval(() => {
      const elapsed = Math.floor((Date.now() - start) / 1000);
      const m = String(Math.floor(elapsed / 60)).padStart(2, "0");
      const s = String(elapsed % 60).padStart(2, "0");
      setTime(`${m}:${s}`);
    }, 1000);
    return () => clearInterval(tk);
  }, []);

  // Caret blink
  useEffect(() => {
    const tk = setInterval(() => setBlink((b) => !b), 530);
    return () => clearInterval(tk);
  }, []);

  // Drive script
  useEffect(() => {
    let cancelled = false;
    let scheduled = [];

    function reset() {
      setStatuses({});
      setLogs([]);
      setPanels({ ceo: "", pm: "", tech_lead: "", readme: "", main: "", math: "" });
      setCommand("");
    }

    function run() {
      reset();
      SCRIPT.forEach((evt) => {
        const handle = setTimeout(() => {
          if (cancelled) return;
          if (evt.restart) { run(); return; }
          if (evt.cmd) typeCommand(evt.cmd);
          if (evt.log) setLogs((prev) => [...prev, ...evt.log]);
          if (evt.set) setStatuses((prev) => ({ ...prev, ...evt.set }));
          if (evt.panels) setPanels((prev) => ({ ...prev, ...evt.panels }));
        }, evt.t / speed);
        scheduled.push(handle);
      });
    }

    function typeCommand(text) {
      let i = 0;
      const tk = setInterval(() => {
        if (cancelled) { clearInterval(tk); return; }
        i++;
        setCommand(text.slice(0, i));
        if (i >= text.length) clearInterval(tk);
      }, 24 / speed);
      scheduled.push(tk);
    }

    run();
    return () => {
      cancelled = true;
      scheduled.forEach((h) => { clearTimeout(h); clearInterval(h); });
    };
  }, [speed]);

  return (
    <div className="tui-frame scanlines">
      <div className="tui-chrome">
        <span className="tui-chrome__dot tui-chrome__dot--r" />
        <span className="tui-chrome__dot tui-chrome__dot--y" />
        <span className="tui-chrome__dot tui-chrome__dot--g" />
        <span className="tui-chrome__title">cortex — ~/projects</span>
      </div>
      <div className="tui-body">
        <PipelineBar statuses={statuses} />
        <div className="tui-main">
          <div className="tui-main__left">
            <div className="tui-section-label">Agents</div>
            <div className="tui-agents-grid">
              <AgentPanel id="ceo" title="ceo" body={panels.ceo} status={statuses.CEO === "done" ? "done" : panels.ceo ? "running" : "idle"} color="var(--accent)" />
              <AgentPanel id="pm" title="pm" body={panels.pm} status={statuses.PM === "done" ? "done" : panels.pm ? "running" : "idle"} color="var(--accent)" />
              <AgentPanel id="tech_lead" title="tech_lead" body={panels.tech_lead} status={statuses.TECH_LEAD === "done" ? "done" : panels.tech_lead ? "running" : "idle"} color="var(--accent)" />
              <AgentPanel id="readme" title="developer:README.md" body={panels.readme} status={panels.readme.includes("DONE") ? "done" : panels.readme ? "running" : "idle"} color="var(--accent-2)" />
              <AgentPanel id="main" title="developer:main.go" body={panels.main} status={panels.main.includes("DONE") ? "done" : panels.main ? "running" : "idle"} color="var(--accent-2)" />
              <AgentPanel id="math" title="developer:math/rand" body={panels.math} status={panels.math.includes("DONE") ? "done" : panels.math ? "running" : "idle"} color="var(--accent-2)" />
            </div>
          </div>
          <div className="tui-main__right">
            <div className="tui-section-label">Logs</div>
            <LogPane lines={logs} />
          </div>
        </div>
        <div className="tui-section-label tui-section-label--cmd">Command</div>
        <CommandBar command={command} blink={blink} />
        <StatusBar time={time} />
      </div>
    </div>
  );
}

window.TUIDemo = TUIDemo;
