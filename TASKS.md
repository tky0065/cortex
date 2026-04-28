# Cortex — Task List

Legend: 🔴 Critical · 🟠 High · 🟡 Medium · 🟢 Nice to have

---

## Phase 1 — Fondations (Cargo + CLI skeleton)

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 1 | 🔴 | Ajouter les dépendances dans `Cargo.toml` (`rig-core`, `tokio`, `clap`, `ratatui`, `crossterm`, `tui-input`, `serde`, `toml`) — retirer `indicatif` | ✅ Done |
| 2 | 🔴 | Implémenter `main.rs` : parsing clap (`cortex`, `cortex start "<idea>"`, `--auto`) | ✅ Done |
| 3 | 🔴 | Implémenter `repl.rs` : boucle REPL interactive avec dispatch des slash commands (`/start`, `/status`, `/abort`, `/help`, `/config`, `/logs`) | ✅ Done |
| 4 | 🔴 | Implémenter `config.rs` : chargement de `~/.cortex/config.toml` avec valeurs par défaut (inclut `max_parallel_workers`) | ✅ Done |

---

## Phase 2 — Outils MCP (Tools)

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 5 | 🔴 | `tools/filesystem.rs` : read/write sandboxé dans le répertoire projet (validation anti path-traversal `../`) | ✅ Done |
| 6 | 🔴 | `tools/terminal.rs` : exécution de commandes avec allowlist stricte (`cargo`, `go`, `npm`, `pip`, `git`, `docker`) + capture stdout/stderr | ✅ Done |
| 7 | 🟠 | `tools/web_search.rs` : recherche web pour la documentation à jour | ✅ Done |

---

## Phase 3 — Gestion du contexte

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 8 | 🔴 | `context/mod.rs` : injection sélective du contexte par agent (chaque agent reçoit uniquement ses fichiers requis) | ✅ Done |
| 9 | 🔴 | Implémenter le budget de tokens par appel agent (configurable via `max_tokens_per_call`) | ✅ Done |
| 10 | 🟠 | `context/compressor.rs` : compression/résumé des outputs longs avant de les passer en downstream | ✅ Done |

---

## Phase 4 — Providers LLM

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 11 | 🔴 | `providers/ollama.rs` : intégration Ollama via API OpenAI-compatible (inférence locale) | ✅ Done |
| 12 | 🟠 | `providers/openrouter.rs` : intégration OpenRouter (fallback remote) | ✅ Done |
| 13 | 🟡 | Support Groq et Together AI comme providers additionnels | ✅ Done |
| 14 | 🔴 | Assignment de modèle par agent via config (`models.ceo`, `models.developer`, etc.) | ✅ Done |

---

## Phase 5 — Infrastructure Multi-Workflows

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 15 | 🔴 | Définir le trait `Workflow` dans `src/workflows/mod.rs` : `name()`, `phases()`, `agents()`, `tools()` | ✅ Done |
| 16 | 🔴 | Rendre l'orchestrateur générique sur `dyn Workflow` — pas de couplage dur au workflow `dev` | ✅ Done |
| 17 | 🔴 | Routing CLI : `cortex run --workflow <name>` + dispatch REPL `/run <name> "<prompt>"` | ✅ Done |
| 18 | 🟠 | Adapter la TUI pour afficher dynamiquement les agents et phases du workflow actif (pas hardcodé sur les 6 agents dev) | ✅ Done |

---

## Phase 6 — Workflow `dev` (Agents développement)

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 19 | 🔴 | `workflows/dev/agents/ceo.rs` : validation de l'idée, définition du brief | ✅ Done |
| 20 | 🔴 | `workflows/dev/agents/pm.rs` : génération de `specs.md` | ✅ Done |
| 21 | 🔴 | `workflows/dev/agents/tech_lead.rs` : génération de `architecture.md` | ✅ Done |
| 22 | 🔴 | `workflows/dev/agents/developer.rs` : worker async — un fichier source par invocation | ✅ Done |
| 23 | 🔴 | `workflows/dev/agents/qa.rs` : worker async — teste un module indépendamment | ✅ Done |
| 24 | 🔴 | `workflows/dev/agents/devops.rs` : artefacts déploiement en parallèle + `git commit` | ✅ Done |
| 25 | 🟠 | System prompts dans `workflows/dev/prompts/<role>.md` pour chaque agent | ✅ Done |

---

## Phase 7 — Workflow `marketing` (Campagne & Test Produit)

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 26 | 🟠 | `workflows/marketing/mod.rs` : implémenter le trait `Workflow` pour le workflow marketing | ✅ Done |
| 27 | 🟠 | `workflows/marketing/agents/strategist.rs` : analyse produit, positionnement, UVP, canaux → `strategy.md` | ✅ Done |
| 28 | 🟠 | `workflows/marketing/agents/copywriter.rs` : rédaction des textes bruts (accroches, posts, CTA) | ✅ Done |
| 29 | 🟠 | `workflows/marketing/agents/social_media_manager.rs` : adaptation par canal (LinkedIn, Twitter/X, Instagram) + calendrier | ✅ Done |
| 30 | 🟠 | `workflows/marketing/agents/analyst.rs` : KPIs, A/B tests suggérés, analyse retours | ✅ Done |
| 31 | 🟠 | Exécution parallèle : Copywriter ‖ Analyst dès `strategy.md` disponible | ✅ Done |
| 32 | 🟠 | System prompts dans `workflows/marketing/prompts/<role>.md` | ✅ Done |
| 33 | 🟡 | Output structuré : `posts/linkedin.md`, `posts/twitter_thread.md`, `posts/instagram.md`, `calendar.md`, `metrics.md` | ✅ Done |

---

## Phase 8 — Workflow `prospecting` (Prospection Freelance)

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 34 | 🟠 | `workflows/prospecting/mod.rs` : implémenter le trait `Workflow` + parsing du profil utilisateur (`profile.toml`) | ✅ Done |
| 35 | 🟠 | `workflows/prospecting/agents/researcher.rs` : recherche web de prospects selon critères (secteur, taille, signaux) | ✅ Done |
| 36 | 🟠 | `workflows/prospecting/agents/profiler.rs` : worker async — analyse individuelle par prospect (activité, besoins, score) | ✅ Done |
| 37 | 🟠 | `workflows/prospecting/agents/copywriter.rs` : worker async — email personnalisé par prospect ancré sur ses signaux | ✅ Done |
| 38 | 🟠 | `workflows/prospecting/agents/outreach_manager.rs` : tri par score, export CSV ou envoi SMTP | ✅ Done |
| 39 | 🟠 | Exécution parallèle : N Profiler workers + N Copywriter workers (un par prospect) | ✅ Done |
| 40 | 🔴 | `tools/email.rs` : outil SMTP avec mode `--dry-run` par défaut, `--send` pour envoi réel après confirmation utilisateur | ✅ Done |
| 41 | 🔴 | Guardrails éthiques : uniquement données publiques, confirmation explicite avant envoi, mention RGPD dans output | ✅ Done |
| 42 | 🟠 | System prompts dans `workflows/prospecting/prompts/<role>.md` | ✅ Done |
| 43 | 🟡 | Output : `prospects_profiles.json`, `emails/<prospect>.md`, `outreach_report.md` | ✅ Done |

---

## Phase 9 — Orchestrateur Principal (cœur du système)

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 22 | 🔴 | `orchestrator.rs` : machine à états globale — définir les phases et les transitions (`PhaseComplete`, `FileReady`, `TestsPassed`) | ✅ Done |
| 23 | 🔴 | Bus d'événements interne : `mpsc::channel` pour collecter les outputs agents → orchestrateur | ✅ Done |
| 24 | 🔴 | `broadcast::channel` pour diffuser `specs.md` et `architecture.md` à N agents simultanément | ✅ Done |
| 25 | 🔴 | Exécution parallèle Phase 2 : `tokio::join!(tech_lead, devops_skeleton)` dès `specs.md` disponible | ✅ Done |
| 26 | 🔴 | Pool de workers Developer : `tokio::spawn` par fichier source + `Semaphore` pour limiter la concurrence (configurable `max_parallel_workers`) | ✅ Done |
| 27 | 🔴 | QA workers parallèles : un `tokio::spawn` par module à tester, results collectés via channel | ✅ Done |
| 28 | 🔴 | Boucle QA ↔ Developer avec compteur d'itérations et cap configurable (`max_qa_iterations`) | ✅ Done |
| 29 | 🔴 | Exécution parallèle Phase 5 : DevOps génère `Dockerfile`, `docker-compose.yml`, `README.md` en parallèle puis `git commit` séquentiel | ✅ Done |
| 30 | 🔴 | Mode `--interactive` : pauses après `specs.md`, `architecture.md`, premier build réussi (suspend le bus d'événements) | ✅ Done |
| 31 | 🟠 | Mode `--auto` : workflow 100% autonome sans interruption | ✅ Done |
| 32 | 🟠 | Gestion de l'abort : signal `tokio::CancellationToken` propagé à toutes les tâches actives | ✅ Done |

---

## Phase 10 — Interface Ratatui (TUI)

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 33 | 🔴 | `tui/mod.rs` : setup ratatui + crossterm (alternate screen, raw mode, event loop async compatible tokio) | ✅ Done |
| 34 | 🔴 | `tui/layout.rs` : layout principal — 4 zones fixes (pipeline bar, agents actifs, logs panel, input bar) + status bar | ✅ Done |
| 35 | 🔴 | `tui/widgets/pipeline.rs` : barre de statut des 6 agents (`✓` terminé · `●` actif · `◌` en attente · `✗` erreur) mise à jour via channel orchestrateur | ✅ Done |
| 36 | 🔴 | `tui/widgets/agent_panel.rs` : blocs dynamiques par agent actif (apparaît/disparaît) avec barre de progression et dernière ligne streamée | ✅ Done |
| 37 | 🔴 | `tui/widgets/logs.rs` : panel logs horodaté scrollable, filtrable par agent | ✅ Done |
| 38 | 🔴 | `tui/widgets/input.rs` : barre de saisie (`tui-input`) pour slash commands — visible en REPL, masquée en `--auto` | ✅ Done |
| 39 | 🔴 | `tui/events.rs` : channel `TuiEvent` reçu depuis orchestrateur → renderer (AgentStarted, TokenChunk, AgentDone, PhaseComplete, Error) | ✅ Done |
| 40 | 🟠 | Avancement des barres de progression au rythme des tokens streamés depuis le provider LLM | ✅ Done |
| 41 | 🟠 | Affichage multi-workers en parallèle : N blocs Developer/QA simultanés avec progression individuelle | ✅ Done |
| 42 | 🟠 | Popup de confirmation ratatui en mode `--interactive` : `[C]ontinuer [E]diter [A]borter` aux points de pause | ✅ Done |
| 43 | 🟠 | Écran récapitulatif final : arborescence fichiers créés, tests pass/fail, hash Git, commande de lancement | ✅ Done |
| 44 | 🟡 | Status bar : provider actif, modèle, tokens consommés total, durée elapsed | ✅ Done |

---

## Phase 11 — Qualité & Robustesse

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 45 | 🟠 | Tests unitaires pour le sandbox filesystem (valider le rejet de `../`) | ✅ Done |
| 46 | 🟠 | Tests unitaires pour l'allowlist terminal | ✅ Done |
| 47 | 🟠 | Tests du bus d'événements orchestrateur : vérifier les transitions de phase sous charge parallèle | ✅ Done |
| 48 | 🟠 | Tests ratatui headless : vérifier le rendu des widgets sans terminal réel (`ratatui::backend::TestBackend`) | ✅ Done |
| 49 | 🟡 | `cortex resume <project-dir>` : reprise d'un workflow interrompu | ✅ Done |
| 50 | 🟡 | Flag `--verbose` : log complet des prompts/réponses agents dans `cortex.log` | ✅ Done |
| 51 | 🟢 | Génération de config CI `.github/workflows/` par le DevOps agent | ✅ Done |

---

## Phase 12 — Extensions & Correctifs

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 52 | 🟠 | `tools/web_search.rs` : câbler Brave Search API via `reqwest` (clé `WEB_SEARCH_API_KEY`) | ✅ Done |
| 53 | 🟠 | `tools/email.rs` : implémenter SMTP live via `lettre` (vars `SMTP_HOST/USER/PASS/PORT`) | ✅ Done |
| 54 | 🔴 | `workflows/dev/mod.rs` : remplacer le polling 200ms dans `wait_for_resume` par écoute channel | ✅ Done |
| 55 | 🟠 | `workflows/mod.rs` + `orchestrator.rs` : ajouter `resume_rx` dans `RunOptions` et le passer depuis l'orchestrateur | ✅ Done |
| 56 | 🟡 | `workflows/prospecting/mod.rs` : charger `profile.toml` du répertoire projet pour enrichir le prompt Researcher | ✅ Done |
| 57 | 🟠 | Workflow `code-review` : Reviewer → Security ‖ Performance → Reporter, avec `tokio::join!` | ✅ Done |
| 58 | 🟠 | `providers/mod.rs` : ajouter les rôles `reviewer`, `security`, `performance`, `reporter` | ✅ Done |

---

## Phase 13 — Correctifs post-audit

| # | Priorité | Tâche | Statut |
|---|----------|-------|--------|
| 59 | 🔴 | `providers/mod.rs` : fonction `complete()` qui parse le préfixe provider (`ollama/`, `openrouter/`, `groq/`, `together/`) et route vers le bon client rig — remplace le `rig_ollama::Client::new()` hardcodé dans tous les agents | ✅ Done |
| 60 | 🟠 | `repl.rs` : ajouter le handler `/resume <project-dir>` manquant dans le dispatcher REPL | ✅ Done |
| 61 | 🟡 | `providers/mod.rs` : ajouter les rôles marketing/prospecting (`strategist`, `copywriter`, `analyst`, `social_media_manager`, `researcher`, `profiler`, `outreach_manager`) dans `model_for_role()` — corriger les agents qui appelaient `model_for_role("developer", ...)` | ✅ Done |

```
1→2→4          Cargo.toml + main.rs + config.rs (skeleton qui compile)
    ↓
15→16→17       trait Workflow + orchestrateur générique + routing CLI
               (fondation multi-workflow avant tout agent)
    ↓
33→34→38→39    TUI ratatui : setup + layout + events channel
               (tôt pour voir le workflow s'animer dès les premiers agents)
    ↓
3              repl.rs branché sur la TUI input bar
    ↓
5→6            outils sandboxés filesystem + terminal
    ↓
40             tools/email.rs + guardrails RGPD (41)
    ↓
11→14          provider Ollama + model assignment
    ↓
8→9            context management
    ↓
22→23→24       bus d'événements orchestrateur → canal TuiEvent → renderer
    ↓
35→36→37       widgets pipeline bar + agent panels + logs
    ↓
── WORKFLOW DEV ──
19→20→21       CEO + PM + Tech Lead (séquentiels)
    ↓
25→22→23       parallélisme Dev workers + QA workers + boucle fix
    ↓
24             DevOps final
    ↓
── WORKFLOW MARKETING ──
26→27          trait + Stratège (premier agent, valide l'infra)
    ↓
28→30→31       Copywriter ‖ Analyst en parallèle
    ↓
29→32→33       Social Media Manager + prompts + output
    ↓
── WORKFLOW PROSPECTING ──
34→35          trait + Researcher
    ↓
36→37→39       Profiler ‖ Copywriter workers en parallèle
    ↓
38→42→43       Outreach Manager + prompts + output
    ↓
── FINALISATION ──
18→44          TUI dynamique par workflow + status bar
    ↓
30→31→32       modes interactif/auto + abort + CancellationToken
    ↓
45→46→47→48    tests sécurité + concurrence + TUI headless
```
