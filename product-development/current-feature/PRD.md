# PRD — Cortex: Agentic Multi-Model Software Company CLI

**Status:** Draft  
**Date:** 2026-04-27  
**Author:** Product Team

---

## 1. Problem Statement

### The Core Problem

Developers and entrepreneurs with software ideas face a steep, time-consuming path from concept to working code. Even experienced engineers must context-switch constantly across roles (architecture decisions, writing code, writing tests, configuring deployment) that each require different mental modes and expertise. For non-engineers or small teams, this gap is often insurmountable without hiring or outsourcing.

Existing AI coding assistants (Copilot, Cursor, etc.) are reactive — they assist a human who must still orchestrate every step. There is no tool that autonomously coordinates the full lifecycle from *"I have an idea"* to *"here is a deployable repository."*

### User Needs

| User | Need |
|------|------|
| Solo developer / indie hacker | Ship a working prototype fast without writing boilerplate |
| Technical founder | Validate product ideas with runnable code before committing engineering resources |
| Developer learning a new stack | Get an opinionated, working reference project in an unfamiliar language/framework |
| Team lead / architect | Automate the tedious scaffolding phase of a new service so the team can focus on business logic |

### Jobs to Be Done

- **Core JTBD:** When I have a software idea, I want to turn it into a working, deployable codebase without doing all the orchestration work myself, so I can ship faster and focus on what matters.
- **Secondary JTBD:** When I start a project in an unfamiliar stack, I want expert-level architecture decisions applied automatically so I don't make costly structural mistakes early.
- **Tertiary JTBD:** When I run cortex, I want to keep my data local and avoid vendor lock-in so I can use powerful open-source models without recurring API costs.

---

## 2. Product Vision

Cortex is a **CLI-first agentic system** that simulates a complete software development company. The user provides a single natural-language prompt; Cortex orchestrates a team of specialized AI agents — each with a defined role, toolset, and assigned model — to deliver a fully functional Git repository containing source code, tests, and deployment configuration.

**Tagline:** *Your entire team, in one command.*

Cortex n'est pas limité au développement logiciel. Il expose une **architecture de workflows extensible** : chaque workflow est un ensemble d'agents spécialisés, d'outils et d'un orchestrateur dédié. Le workflow `dev` (équipe de développement) est le premier ; d'autres workflows (`marketing`, `prospecting`, etc.) s'y ajoutent avec la même infrastructure d'orchestration et de TUI.

---

## 3. Scope

### In Scope (v1.0)

**Infrastructure commune (tous les workflows) :**
- Architecture de workflows extensible : chaque workflow est un module Rust indépendant avec ses agents, ses outils et son orchestrateur
- Sélection du workflow au lancement : `cortex run --workflow dev "mon idée"` ou via le REPL `/run dev "..."`
- TUI ratatui partagée, adaptée dynamiquement au workflow actif (agents et phases affichés varient)
- Orchestrateur parallèle commun, providers LLM communs, système de contexte commun

**Workflow `dev` — Équipe de développement logiciel :**
- Six agents : CEO, PM, Tech Lead, Developer, QA, DevOps
- Output : code source, `specs.md`, `architecture.md`, tests, `Dockerfile`, `docker-compose.yml`, Git repo

**Workflow `marketing` — Campagne marketing & test produit :**
- Agents : Stratège, Copywriter, Social Media Manager, Analyst
- Output : stratégie de contenu, posts réseaux sociaux (LinkedIn, Twitter/X, Instagram), analyse de ciblage, plan de campagne

**Workflow `prospecting` — Prospection commerciale (freelances & agences) :**
- Agents : Researcher, Profiler, Copywriter, Outreach Manager
- Output : liste de prospects qualifiés, emails de prospection personnalisés par secteur/activité, proposition de services adaptée
- Outils MCP supplémentaires : recherche web, scraping LinkedIn public, envoi d'emails (SMTP)

### Out of Scope (v1.0)

- GUI ou interface web
- Collaboration multi-utilisateur en temps réel
- Déploiement cloud autonome (provisioning infrastructure réelle)
- Fine-tuning ou entraînement de modèles
- Marketplace de workflows/plugins
- Tier SaaS payant/hébergé
- Kubernetes manifests (stretch goal)
- Intégration CRM (stretch goal pour `prospecting`)

---

## 4. Agent Specifications

### 4.1 CEO — Strategy & Validation

**Role:** Entry point of every project. Analyzes the raw user idea, assesses feasibility, and defines the high-level product vision passed to all downstream agents.

**Inputs:** Raw user prompt  
**Outputs:** Validated project brief (vision, scope boundaries, success definition)  
**Tools available:** Web search (for competitive landscape awareness)  
**Validation gate:** If the idea is unclear or infeasible, the CEO asks the user clarifying questions before proceeding.

---

### 4.2 Product Manager — Specification

**Role:** Translates the CEO's brief into a structured `specs.md` document that becomes the ground truth for all other agents.

**Inputs:** CEO brief  
**Outputs:** `specs.md` — user stories, feature list, acceptance criteria, non-functional requirements  
**Tools available:** Filesystem write  
**Key constraint:** Must produce a document concise enough to fit downstream agent context windows without truncation.

---

### 4.3 Tech Lead — Architecture

**Role:** Reads `specs.md` and produces `architecture.md` — the technical blueprint the Developer follows.

**Inputs:** `specs.md`  
**Outputs:** `architecture.md` — chosen tech stack, directory structure, data models/schemas, API contracts (REST endpoints or other interfaces)  
**Tools available:** Web search (fetch up-to-date docs for chosen frameworks), filesystem write  
**Key decisions:** Language/framework selection, database choice, folder layout, naming conventions.

---

### 4.4 Developer — Implementation

**Role:** The primary code producer. Creates all source files by following `architecture.md` and `specs.md` strictly.

**Inputs:** `specs.md`, `architecture.md`  
**Outputs:** All source files written to the project directory  
**Tools available:** Filesystem read/write, terminal execution (for `cargo check`, `go build`, `npm install`, etc.)  
**Constraint:** Writes files incrementally (one file at a time) to stay within context limits. Receives QA bug reports and iterates until the build passes.

---

### 4.5 QA Engineer — Testing & Verification

**Role:** Reviews the Developer's output, writes unit and integration tests, executes them via the Terminal tool, and reports failures back to the Developer.

**Inputs:** Source files written by Developer  
**Outputs:** Test files, structured bug reports with exact error messages  
**Tools available:** Filesystem read/write, terminal execution (run test commands, capture output)  
**Loop condition:** QA ↔ Developer loop iterates until all tests pass or a maximum iteration count is reached.

---

### 4.6 DevOps — Infrastructure & Delivery

**Role:** Final stage. Produces all deployment artifacts and commits the entire project to a local Git repository.

**Inputs:** Source files, `architecture.md`  
**Outputs:** `Dockerfile`, `docker-compose.yml`, `.github/workflows/` CI config (optional), `README.md`, initialized Git repo with initial commit  
**Tools available:** Filesystem read/write, terminal execution (`git init`, `git add`, `git commit`, `docker build` dry-run)

---

## 4b. Architecture Multi-Workflows

### Principe général

Chaque workflow est un **module Rust** dans `src/workflows/<nom>/` qui implémente un trait commun `Workflow`. L'orchestrateur principal est générique : il reçoit un workflow, découvre ses phases et ses règles de dépendance, et les exécute avec les mêmes primitives tokio (spawn, join, channels).

```rust
pub trait Workflow: Send + Sync {
    fn name(&self) -> &str;
    fn phases(&self) -> Vec<Phase>;          // phases ordonnées avec deps
    fn agents(&self) -> Vec<Box<dyn Agent>>; // agents du workflow
    fn tools(&self) -> Vec<Box<dyn Tool>>;   // outils MCP requis
}
```

Commande d'invocation :
```
cortex run --workflow dev        "Je veux un microservice Go"
cortex run --workflow marketing  "Lancer mon SaaS de gestion de factures"
cortex run --workflow prospecting "Je suis dev freelance React, trouver des prospects PME e-commerce"
```

Ou via le REPL :
```
/run dev        "Je veux un microservice Go"
/run marketing  "Lancer mon SaaS de gestion de factures"
/run prospecting "Je suis dev freelance React"
```

---

## 4c. Workflow `marketing` — Campagne & Test Produit

### Vue d'ensemble

Le workflow `marketing` orchestre une équipe d'agents pour produire une stratégie de contenu complète et des assets prêts à publier sur les réseaux sociaux, à partir d'une description de produit ou d'idée.

### Agents

| Agent | Rôle |
|-------|------|
| **Stratège** | Analyse l'idée, définit le positionnement, la cible, la proposition de valeur unique (UVP), et les canaux prioritaires |
| **Copywriter** | Rédige les textes : accroches, posts, threads, captions, call-to-action — adaptés au ton de chaque canal |
| **Social Media Manager** | Adapte les contenus aux formats de chaque plateforme (LinkedIn long-form, Twitter/X thread, Instagram caption + hashtags) et planifie le calendrier editorial |
| **Analyst** | Suggère des métriques de suivi (impressions, CTR, conversions), des A/B tests à réaliser, et analyse les retours si des données sont fournies |

### Workflow

```
User input (description produit/service)
     │
     ▼
[Stratège] Positionnement + cible + canaux  →  strategy.md
     │
     ├──────────────────────┐
     ▼                      ▼
[Copywriter]           [Analyst]            ← en parallèle
textes bruts           métriques & A/B tests
     │                      │
     └──────────┬───────────┘
                ▼
[Social Media Manager] Adaptation par canal + calendrier
                ↓
Output : posts/ (LinkedIn, Twitter, Instagram), strategy.md, calendar.md
```

### Outils MCP requis

| Outil | Usage |
|-------|-------|
| Filesystem read/write | Écriture des fichiers de contenu |
| Web search | Analyse concurrentielle, tendances du secteur, hashtags populaires |

### Output

```
output/
  strategy.md           — positionnement, cible, UVP, canaux
  posts/
    linkedin.md         — post(s) LinkedIn long-form
    twitter_thread.md   — thread Twitter/X
    instagram.md        — caption + hashtags
  calendar.md           — planning de publication sur 4 semaines
  metrics.md            — KPIs à suivre, A/B tests suggérés
```

---

## 4d. Workflow `prospecting` — Prospection Commerciale Freelance

### Vue d'ensemble

Le workflow `prospecting` est conçu pour les **freelances et agences** qui veulent contacter des prospects qualifiés de façon autonome et personnalisée. À partir d'un profil utilisateur (compétences, stack, type de missions) et de critères de ciblage (secteur, taille entreprise, signaux d'activité), il produit une liste de prospects et des emails personnalisés prêts à envoyer.

### Agents

| Agent | Rôle |
|-------|------|
| **Researcher** | Recherche des prospects via le web (LinkedIn public, sites entreprises, annuaires) selon les critères de ciblage fournis |
| **Profiler** | Analyse chaque prospect : secteur, taille, activité récente (offres d'emploi, posts LinkedIn, actualités), besoins probables |
| **Copywriter** | Rédige un email de prospection personnalisé par prospect, ancré sur ses signaux d'activité et ses besoins détectés |
| **Outreach Manager** | Consolide la liste, priorise les prospects par score de pertinence, prépare l'envoi (SMTP ou export CSV) |

### Workflow

```
User input (profil freelance + critères de ciblage)
     │
     ▼
[Researcher] Scraping + recherche web  →  prospects_raw.json
     │
     ▼  (N prospects en parallèle)
[Profiler × N]  Analyse individuelle   →  prospects_profiles.json
     │
     ▼  (N copywriters en parallèle)
[Copywriter × N]  Email personnalisé   →  emails/<prospect>.md
     │
     ▼
[Outreach Manager] Score + tri + envoi ou export
     │
     ▼
Output : liste priorisée + emails + rapport d'envoi
```

### Outils MCP requis

| Outil | Usage |
|-------|-------|
| Web search | Recherche de prospects, scraping LinkedIn public, actualités entreprise |
| Filesystem read/write | Stockage des profils et emails générés |
| SMTP / Email tool | Envoi des emails (optionnel — peut exporter CSV à la place) |

### Données d'entrée utilisateur

```toml
[profile]
skills    = ["React", "TypeScript", "Node.js"]
services  = ["refonte frontend", "audit performance", "mise en place design system"]
rate      = "600€/jour"

[targeting]
sectors   = ["e-commerce", "SaaS B2B", "fintech"]
company_size = "10-200 salariés"
signals   = ["offres d'emploi React", "levée de fonds récente", "croissance équipe tech"]
geography = "France"
count     = 20  # nombre de prospects à trouver
```

### Output

```
output/
  prospects_profiles.json   — liste enrichie avec score de pertinence
  emails/
    acme_corp.md            — email personnalisé pour Acme Corp
    startup_xyz.md          — email personnalisé pour Startup XYZ
    ...
  outreach_report.md        — résumé : N prospects trouvés, N emails générés, N envoyés
```

### Règles éthiques & légales

- Uniquement des données **publiquement accessibles** (pas de contournement d'authentification)
- Respect du RGPD : les données personnelles sont traitées localement, non transmises à des tiers
- Option `--dry-run` : génère les emails sans les envoyer (défaut)
- L'envoi réel nécessite une confirmation explicite de l'utilisateur (`--send`)

---

## 5. Orchestrateur Principal & Workflow Parallèle

### 5.1 Orchestrateur Central

L'orchestrateur est le cœur de Cortex. C'est un composant Rust unique (`orchestrator.rs`) qui :

- Maintient la **machine à états globale** du projet (phase courante, état de chaque agent)
- Décide **quels agents peuvent tourner en parallèle** selon les dépendances entre phases
- Gère les **canaux de communication** entre agents (via `tokio::sync` — channels, broadcast)
- Collecte les outputs et les redistribue comme inputs aux agents suivants
- Applique les **règles de contrôle de flux** : pauses interactives, cap d'itérations QA, abort

L'orchestrateur ne contient aucune logique métier — il délègue tout aux agents et aux outils.

### 5.2 Workflow — Phases Séquentielles & Parallèles

Certaines phases sont strictement séquentielles (dépendances de données), d'autres peuvent tourner en parallèle dès que leurs inputs sont disponibles.

```
User Prompt
     │
     ▼
┌─────────────────────────────────────┐
│  PHASE 1 — SÉQUENTIELLE             │
│  [CEO] Validate & scope idea        │
│        ↓                            │
│  [PM] Generate specs.md             │
└─────────────────────────────────────┘
     │
     ▼  specs.md disponible
┌─────────────────────────────────────┐
│  PHASE 2 — PARALLÈLE                │
│  [Tech Lead] architecture.md        │  ← peut démarrer dès specs.md
│  [DevOps]   infra skeleton          │  ← peut préparer Dockerfile template
│       (tous deux lisent specs.md)   │
└─────────────────────────────────────┘
     │
     ▼  architecture.md disponible
┌─────────────────────────────────────┐
│  PHASE 3 — PARALLÈLE                │
│  [Developer] module A  (tokio task) │
│  [Developer] module B  (tokio task) │  ← N workers selon complexité
│  [Developer] module C  (tokio task) │
│       (chaque worker = un fichier)  │
└─────────────────────────────────────┘
     │
     ▼  tous les fichiers écrits
┌─────────────────────────────────────┐
│  PHASE 4 — PARALLÈLE                │
│  [QA] unit tests          (task 1)  │
│  [QA] integration tests   (task 2)  │  ← tests indépendants en parallèle
│  [Developer] fix loop si bugs       │
└─────────────────────────────────────┘
     │
     ▼  tests passent
┌─────────────────────────────────────┐
│  PHASE 5 — PARALLÈLE                │
│  [DevOps] Dockerfile                │
│  [DevOps] docker-compose.yml        │  ← artefacts indépendants
│  [DevOps] README.md                 │
│  [DevOps] git init + commit         │  ← séquentiel après les 3 ci-dessus
└─────────────────────────────────────┘
     │
     ▼
Final Report
```

### 5.3 Modèle de Concurrence

L'orchestrateur utilise `tokio` pour lancer les agents en tâches concurrentes :

| Pattern | Usage |
|---------|-------|
| `tokio::spawn` | Lancer un agent en tâche indépendante |
| `tokio::join!` | Attendre la fin de plusieurs agents en parallèle |
| `tokio::select!` | Réagir au premier agent qui termine (ex: QA race) |
| `mpsc::channel` | Envoyer les outputs d'un agent vers l'orchestrateur |
| `broadcast::channel` | Diffuser `specs.md` / `architecture.md` à N agents simultanément |
| `Semaphore` | Limiter le nombre de workers Developer concurrents (configurable) |

### 5.4 Règles de Dépendance entre Phases

| Agent | Dépend de | Peut démarrer dès que |
|-------|-----------|----------------------|
| CEO | User prompt | Immédiat |
| PM | CEO output | CEO terminé |
| Tech Lead | `specs.md` | PM terminé |
| DevOps (skeleton) | `specs.md` | PM terminé (parallèle avec Tech Lead) |
| Developer workers | `architecture.md` | Tech Lead terminé |
| QA | Fichier(s) source écrits | Chaque fichier peut être testé dès sa création |
| DevOps (final) | Tous tests passent | QA loop terminé |

**Phase transitions** : l'orchestrateur émet des événements (`PhaseComplete`, `FileReady`, `TestsPassed`) sur un bus interne. Chaque agent est abonné aux événements qui déclenchent son démarrage — pas de polling, pas de sleep.

---

## 6. MCP Integration Requirements

### 6.1 Filesystem Server

- **Read:** Any agent can read files in the current working directory.
- **Write:** PM, Tech Lead, Developer, QA, DevOps can create/overwrite files.
- **Scope restriction:** All read/write operations are sandboxed to the project output directory; no access outside it.

### 6.2 Terminal Server

- **Execute:** Developer, QA, and DevOps can run shell commands.
- **Allowed commands (allowlist):** Build tools (`cargo`, `go`, `npm`, `pip`, `mvn`), test runners, `git`, `docker` (dry-run/build only).
- **Blocked commands:** Any command that accesses the network beyond the project scope, or modifies system state outside the project directory.
- **Output capture:** stdout and stderr are returned to the requesting agent as plain text for analysis.

---

## 7. State & Context Management

This is the most critical architectural concern. Naively passing all accumulated context to every agent will exceed LLM context windows on any non-trivial project.

### Requirements

| Requirement | Description |
|-------------|-------------|
| **Selective context injection** | Each agent receives only the documents relevant to its role (e.g., Developer gets `specs.md` + `architecture.md`, not the CEO brief transcript) |
| **File-as-memory** | Generated files (`specs.md`, `architecture.md`) serve as persistent, shared memory — agents read them from disk rather than receiving them in the prompt |
| **Summary compression** | Long agent outputs are summarized before being passed downstream; full content remains on disk |
| **Max token budget per agent call** | Configurable hard limit per agent invocation to prevent runaway inference costs |
| **Iteration state** | The QA ↔ Dev loop tracks iteration count, stopping after a configurable maximum (default: 5) |

---

## 8. LLM Provider Requirements

| Requirement | Detail |
|-------------|--------|
| **Local first** | Ollama support for zero-cost, offline inference (Qwen 2.5 Coder, DeepSeek Coder, Mixtral) |
| **Remote fallback** | OpenAI-compatible API support: OpenRouter, Groq, Together AI |
| **Per-agent model assignment** | Each agent can be assigned a different model in config (e.g., CEO uses a reasoning model, Developer uses a code-optimized model) |
| **No vendor lock-in** | Provider is swappable via config file; no hard dependency on any single provider's SDK beyond `rig-core` |
| **Streaming output** | Token streaming to the terminal for long-running generations so the user sees progress |

---

## 9. CLI User Experience Requirements

### 9.1 Entry Modes

Cortex supports two entry modes, like `claude` or `opencode`:

**Mode A — REPL interactif (défaut)**
```
$ cortex
cortex v0.1.0 — type /help for commands
❯
```
L'utilisateur arrive dans une session persistante avec des commandes slash. C'est le mode principal.

**Mode B — One-shot CLI (scripting & CI)**
```
$ cortex start "a booking microservice in Go" --auto
```
Lance le workflow complet sans interaction, produit le rapport final et quitte. Utile en scripts ou pipelines CI.

Les deux modes partagent le même moteur : Mode B est un wrapper qui injecte l'idée et passe `--auto`.

---

### 9.2 Commandes Slash (REPL)

| Commande | Description |
|----------|-------------|
| `/start "<idée>"` | Lance un nouveau projet depuis le REPL |
| `/start "<idée>" --auto` | Lance en mode autonome complet sans pauses |
| `/status` | Affiche la phase courante et l'avancement |
| `/resume <project-dir>` | Reprend un projet interrompu |
| `/config` | Affiche et modifie la configuration provider/modèles |
| `/logs` | Affiche le log verbose de la session en cours |
| `/abort` | Interrompt le workflow en cours et sauvegarde l'état |
| `/help` | Liste toutes les commandes disponibles |

---

### 9.3 Mode Interactif vs Autonome

Le workflow supporte deux niveaux d'autonomie, choisis par flag :

| Flag | Comportement |
|------|-------------|
| `--interactive` (défaut REPL) | Le CLI s'arrête après chaque phase et demande validation : `specs.md généré — continuer ? [Y/n]`. L'utilisateur peut éditer le fichier avant de continuer. |
| `--auto` (défaut one-shot) | Workflow 100% autonome, aucune interruption. Le CLI tourne jusqu'au rapport final. |

La pause interactive se produit à trois points critiques :
1. Après `specs.md` (PM → Tech Lead)
2. Après `architecture.md` (Tech Lead → Developer)
3. Après le premier build réussi (Developer → QA)

---

### 9.4 Interface Ratatui (TUI)

Cortex utilise **ratatui** pour une interface terminal entièrement structurée — pas un simple flux de logs, mais une vraie TUI avec layout fixe mise à jour en temps réel.

#### Layout général

```
┌─ CORTEX ─────────────────────── v0.1.0 ─── Ollama/qwen2.5-coder:32b ─┐
│                                                                         │
│  ┌─ Pipeline ──────────────────────────────────────────────────────┐   │
│  │  ✓ CEO   ✓ PM   ● Tech Lead   ◌ Developer   ◌ QA   ◌ DevOps   │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌─ Agents actifs ─────────────────────────────────────────────────┐   │
│  │  🏗️  Tech Lead   [████████░░░░░░░░]  Generating architecture.md │   │
│  │  💻  Dev/main.go [██████████████░░]  Writing handlers...        │   │
│  │  💻  Dev/db.go   [████░░░░░░░░░░░░]  Writing models...          │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌─ Logs ──────────────────────────────────────────────────────────┐   │
│  │  [14:02:31] PM       specs.md written (1.2 KB)                  │   │
│  │  [14:02:45] TechLead Starting architecture design...            │   │
│  │  [14:03:01] Dev      Spawning 3 workers for Go modules          │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌─ Input ─────────────────────────────────────────────────────────┐   │
│  │  ❯ _                                                            │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────── q:quit ─┘
```

#### Composants de l'interface

| Widget ratatui | Rôle |
|----------------|------|
| **Pipeline bar** | Barre de statut des 6 agents : `✓` terminé · `●` actif · `◌` en attente · `✗` erreur |
| **Agents actifs** | Un bloc par agent en cours — barre de progression + dernière action en streaming |
| **Logs panel** | Flux horodaté des événements orchestrateur (scrollable, filtrable par agent) |
| **Input bar** | Zone de saisie des slash commands ; visible en mode REPL, masquée en `--auto` |
| **Status bar** | Provider actif, modèle, tokens consommés, durée elapsed |

#### Comportement dynamique

- Les blocs **Agents actifs** apparaissent et disparaissent dynamiquement selon les agents en cours (gérés par l'orchestrateur via channel vers le renderer TUI)
- En mode **parallèle** (phase Developer), N blocs de workers s'affichent simultanément avec leur progression individuelle
- Les barres de progression avancent à chaque token streamé depuis le provider LLM
- En mode `--interactive`, une **popup de confirmation** ratatui s'ouvre aux points de pause : `specs.md généré — [C]ontinuer  [E]diter  [A]borter`
- Le rapport final remplace le layout principal par un **écran récapitulatif** : arborescence des fichiers créés, résultat des tests, hash Git, commande de lancement

#### Crates UI

| Crate | Rôle |
|-------|------|
| `ratatui` | Framework TUI principal (widgets, layout, rendering) |
| `crossterm` | Backend terminal cross-platform (events clavier, souris) |
| `tui-input` | Widget input pour la barre de commandes |

`indicatif` est retiré — ratatui gère tout l'affichage.

### 9.5 Configuration

Config file at `~/.cortex/config.toml` (or `$CORTEX_CONFIG`):

```toml
[provider]
default = "ollama"

[models]
ceo       = "ollama/qwen2.5-coder:32b"
pm        = "ollama/qwen2.5-coder:14b"
tech_lead = "ollama/qwen2.5-coder:32b"
developer = "ollama/qwen2.5-coder:32b"
qa        = "ollama/qwen2.5-coder:14b"
devops    = "ollama/qwen2.5-coder:7b"

[limits]
max_qa_iterations = 5
max_tokens_per_call = 8192
```

---

## 10. Success Metrics & Acceptance Criteria

### 10.1 Core Acceptance Criteria (v1.0 Definition of Done)

- [ ] `cortex start "a REST API in Go for user authentication"` completes without human intervention and produces a runnable project.
- [ ] Generated project compiles/builds successfully using the target language's standard build tool.
- [ ] At least one test file is generated and executes (pass or fail) via the QA agent.
- [ ] `Dockerfile` and `docker-compose.yml` are present and syntactically valid.
- [ ] A Git repository is initialized with an initial commit in the output directory.
- [ ] `specs.md` and `architecture.md` are present and non-empty.
- [ ] All file writes are sandboxed to the project output directory.
- [ ] CLI works with at least one Ollama model (local) and one OpenRouter model (remote).

### 10.2 Quality Metrics

| Metric | Target |
|--------|--------|
| End-to-end completion rate (project builds successfully) | ≥ 70% on a benchmark of 10 representative prompts |
| Mean time to completed project (simple API) | < 5 minutes on local hardware with Qwen 2.5 Coder 14B |
| Context window overflow errors | 0 — no agent call should hit a hard context limit |
| Unauthorized filesystem access attempts | 0 |

### 10.3 User Experience Metrics

- User can understand what each agent is doing at any point in time (progress is always visible).
- Error messages are actionable — they tell the user what failed and what to try next.
- Config setup takes < 5 minutes for a developer already running Ollama.

---

## 11. Non-Functional Requirements

| Category | Requirement |
|----------|-------------|
| **Performance** | CLI startup < 100ms; tool call overhead (MCP) < 50ms per call |
| **Safety** | Terminal tool executes only allowlisted commands; no network access from sandboxed tools |
| **Privacy** | When using local Ollama, zero data leaves the machine |
| **Reliability** | Graceful failure at any phase: error is reported, partial output is preserved |
| **Cross-platform** | Builds and runs on macOS, Linux; Windows support is stretch goal |
| **Observability** | `--verbose` flag enables full agent prompt/response logging to `cortex.log` |

---

## 12. Risks & Mitigations

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| LLM output doesn't parse as valid code | High | QA + Dev iteration loop; static syntax check before committing |
| Context window overflow on large projects | High | File-as-memory pattern + selective context injection (§7) |
| Ollama model quality insufficient for complex tasks | Medium | Document recommended minimum model sizes; allow easy model swap |
| MCP terminal tool executes dangerous commands | Low | Strict allowlist; sandbox to project directory |
| QA ↔ Dev loop never converges | Medium | Hard cap on iterations; report partial output with error summary |
| `rig-core` API changes break integrations | Low | Pin dependency version; document upgrade path |

---

## 13. Open Questions

1. **Parallelism:** Should the Developer write multiple files in parallel (separate agent calls)? Risk: harder to maintain consistency. Benefit: significantly faster execution.
2. **Human-in-the-loop checkpoints:** Should the user be able to review and edit `specs.md` or `architecture.md` before the Developer phase begins? This could dramatically improve output quality.
3. **Resume/checkpoint:** Is it critical for v1.0 to support resuming a failed run, or is a clean restart acceptable?
4. **Static analysis tool:** Which Rust-native static analysis library best fits the embedded linter skill? (`tree-sitter`? `ra_ap_syntax`?)
5. **Max QA iterations:** What is the right default for `max_qa_iterations` before the system gives up and reports partial output?

---

## 14. Glossary

| Term | Definition |
|------|------------|
| **Agent** | An LLM instance configured with a system prompt, a set of tools, and an assigned model |
| **Workflow** | A named, self-contained automation unit — a set of agents + tools + orchestration rules implementing a specific use case (`dev`, `marketing`, `prospecting`) |
| **MCP** | Model Context Protocol — a standard for giving LLM agents access to external tools (filesystem, terminal, web) |
| **rig-core** | The Rust LLM framework used to build agents and connect to providers |
| **Ollama** | A local LLM runtime that serves open-source models via an OpenAI-compatible API |
| **Handoff** | The act of passing the output of one agent phase as input to the next |
| **Context budget** | The maximum number of tokens allocated to a single agent LLM call |
| **File-as-memory** | Pattern where generated artifacts (`specs.md`, `architecture.md`, etc.) are stored on disk and re-read by agents rather than carried in the prompt chain |
| **Signal** | Indicator of prospect intent or need (e.g., job posting for a specific technology, recent funding round) used by the `prospecting` workflow |
| **Dry-run** | Execution mode where the `prospecting` workflow generates emails but does not send them — default behavior |
