/* global React */
const { useState: useStateL } = React;

const AGENTS_DATA = [
  { name: "CEO", role: "Frames the brief.", out: "executive brief" },
  { name: "PM", role: "Writes the specs.", out: "specs.md" },
  { name: "Tech Lead", role: "Designs the stack.", out: "architecture.md" },
  { name: "Developer ×N", role: "Writes the code in parallel.", out: "src/*" },
  { name: "QA", role: "Audits and loops fixes.", out: "report (≤5×)" },
  { name: "DevOps", role: "Containerizes and ships.", out: "Dockerfile · git" },
];

function AgentsSection() {
  return (
    <section id="agents">
      <div className="container">
        <div className="section-head">
          <span className="eyebrow"><span className="pulse" />The team</span>
          <h2 className="section-title">Six specialists, one CLI.</h2>
        </div>
        <div className="agents-grid">
          {AGENTS_DATA.map((a, i) => (
            <div className="agent-cell" key={a.name}>
              <span className="agent-cell__num">0{i + 1}</span>
              <span className="agent-cell__name">{a.name}</span>
              <p className="agent-cell__role">{a.role}</p>
              <span className="agent-cell__out">→ {a.out}</span>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

const WORKFLOWS = [
  { name: "dev", desc: "Idea → deployable repo.", cmd: "/start dev" },
  { name: "marketing", desc: "Strategy, copy, KPIs, calendar.", cmd: "/start marketing" },
  { name: "prospecting", desc: "Find prospects, draft personalized outreach.", cmd: "/start prospecting" },
  { name: "code-review", desc: "Quality, security, performance audit.", cmd: "/start code-review" },
  { name: "connect", desc: "Choose from the full provider registry and save auth.", cmd: "/connect" },
  { name: "init", desc: "Scan a project and generate durable AGENTS.md context.", cmd: "cortex init" },
  { name: "skill", desc: "Install reusable instructions for specialized agent behavior.", cmd: "/skill" },
];

function WorkflowsSection() {
  return (
    <section id="workflows">
      <div className="container">
        <div className="section-head">
          <span className="eyebrow"><span className="pulse" />Workflows</span>
          <h2 className="section-title">One CLI. Different teams.</h2>
        </div>
        <div className="wf-list">
          {WORKFLOWS.map((w) => (
            <div className="wf-row" key={w.name}>
              <span className="wf-row__name">/{w.name}</span>
              <p className="wf-row__desc">{w.desc}</p>
              <span className="wf-row__cmd">{w.cmd}</span>
            </div>
          ))}
        </div>
        <div style={{ marginTop: "var(--space-5)" }}>
          <a className="btn" href="docs.html">Full docs →</a>
        </div>
      </div>
    </section>
  );
}

function InstallSection() {
  return (
    <section id="install" className="cta-section">
      <div className="container-narrow">
        <h2 className="cta-title">Your entire team,<br/><span style={{ color: "var(--accent)" }}>in one command.</span></h2>
        <div className="install-block">
          <div>
            <div className="install-label">macOS / Linux</div>
            <CopyChip cmd="curl -fsSL https://raw.githubusercontent.com/tky0065/cortex/main/install.sh | sh" />
          </div>
          <div>
            <div className="install-label">Windows</div>
            <CopyChip cmd='powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/tky0065/cortex/main/install.ps1 | iex"' />
          </div>
        </div>
        <div className="install-actions">
          <a className="btn btn-primary" href="docs.html">Read the docs →</a>
          <a className="btn" href="https://github.com/tky0065/cortex" target="_blank" rel="noopener">GitHub</a>
        </div>
      </div>
    </section>
  );
}

function CopyChip({ cmd }) {
  const [copied, setCopied] = useStateL(false);
  return (
    <button className={`code-chip ${copied ? "copied" : ""}`} onClick={() => {
      navigator.clipboard?.writeText(cmd);
      setCopied(true); setTimeout(() => setCopied(false), 1400);
    }}>
      <span className="prompt">$</span>
      <span className="code-chip__cmd">{cmd}</span>
      <span className="copy">{copied ? "Copied" : "Copy"}</span>
    </button>
  );
}

window.AgentsSection = AgentsSection;
window.WorkflowsSection = WorkflowsSection;
window.InstallSection = InstallSection;
window.CopyChip = CopyChip;
