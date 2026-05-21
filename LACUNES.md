# Lacunes du projet Cortex

## Resume executif

Cortex a deja une base ambitieuse: workflows multiples, TUI, providers, agents personnalisables, reprise de session, web search, skills et publication beta. Les lacunes principales ne sont donc plus des manques de fonctionnalites de base, mais des risques de produit complet: fiabilite des generations, securite des outils, clarte du positionnement, qualite mesurable des outputs, compatibilite provider, et experience d'installation/support.

Le risque central est que Cortex promette "une equipe logicielle en une commande" sans encore definir assez strictement ce qui rend un resultat acceptable, reproductible, securise et maintenable. Le projet gagnerait a passer d'une logique "beaucoup de workflows implementes" a une logique "quelques workflows prouves, mesures et fiables".

## Lacunes critiques

### 1. Absence de criteres de qualite mesurables pour les projets generes
**Statut:** Terminé
**Preuve:** Couvert par `docs/QUALITY_GATE.md` et `evals/dev/acceptance_matrix.toml`, qui définissent une matrice d'acceptation humaine et structurée pour les outputs `dev`.

**Constat:** Le produit vise a generer des depots complets et deployables, mais il n'y a pas de definition testable de "complet", "deployable", "acceptable" ou "production-ready" selon les stacks.

**Pourquoi c'est important:** Sans criteres objectifs, Cortex peut sembler fonctionner parce qu'il produit des fichiers, tout en livrant des projets incomplets, fragiles ou impossibles a maintenir.

**Action recommandee:** Definir une matrice d'acceptation par type de projet: build, tests, lint, README runnable, Docker valide, commandes de lancement, couverture minimale, absence de secrets, absence de TODO bloquants.

### 2. Risque de securite lie aux outils executes depuis des sorties LLM
**Statut:** Terminé
**Preuve:** Couvert par `docs/SECURITY_THREAT_MODEL.md`, la redaction centrale, les garde-fous tools/email/web search/custom validation, et le lot sécurité adversariale avancée: labellisation des résultats web comme contenu externe non fiable, tests d'attaques composées, et rejets updater checksum/archive suspects.

**Constat:** Le PRD mentionne l'allowlist terminal et le sandbox filesystem, mais le produit s'est elargi: web search, fetch URL, email SMTP, update binary, providers remote, custom agents, custom workflows, mentions, skills.

**Pourquoi c'est important:** Plus Cortex accepte de contenu externe et d'instructions personnalisees, plus les risques de prompt injection, exfiltration, execution non desiree et ecriture de fichiers sensibles augmentent.

**Action recommandee:** Formaliser un modele de menace complet et ajouter des tests d'abus: chemins symboliques, URLs malveillantes, prompt injection dans resultats web, workflow custom qui demande des secrets, envoi email accidentel, update compromis.

### 3. Pas de banc d'evaluation reproductible
**Statut:** Terminé
**Preuve:** Couvert par `evals/dev/` (scenarios, acceptance_matrix.toml, check_dev_output.sh) et `evals/run_campaign.sh`, qui permet de lancer tous les scénarios en batch et produit un rapport JSON horodaté dans `evals/runs/`.

**Constat:** Le projet a des tests unitaires, mais il manque un eval harness qui lance Cortex sur des prompts representatifs et mesure la qualite des depots produits.

**Pourquoi c'est important:** Les regressions d'agents et de prompts sont difficiles a detecter avec des tests Rust classiques. Une petite modification de prompt ou provider peut degrader fortement les resultats sans casser la compilation.

**Action recommandee:** Creer un dossier `evals/` avec 10 a 20 scenarios fixes, sorties attendues, commandes de verification et scoring: build pass, tests pass, fichiers attendus, coherence specs/architecture/code.

### 4. Positionnement produit trop large pour une beta fiable
**Statut:** Terminé
**Preuve:** Couvert par `docs/BETA.md`, qui définit le workflow phare, les workflows expérimentaux et les limites beta.

**Constat:** Cortex couvre dev, marketing, prospecting, code-review, custom agents, custom workflows, skills et providers multiples. Cela cree une promesse tres large.

**Pourquoi c'est important:** Une beta qui couvre trop de cas d'usage risque de paraitre superficielle si aucun workflow n'est excellent. Les utilisateurs ne sauront pas quel probleme Cortex resout mieux que Cursor, Claude Code, Copilot ou OpenCode.

**Action recommandee:** Choisir un workflow phare pour la beta publique, probablement `dev` ou `code-review`, et presenter les autres comme experimentaux jusqu'a validation.

## Lacunes importantes

### 5. Strategie provider insuffisamment clarifiee
**Statut:** Terminé
**Preuve:** Couvert par `docs/PROVIDERS.md`, qui documente les niveaux de support, les recommandations modèles et les limites provider.

**Constat:** Le projet supporte plusieurs providers et modes d'auth, mais la documentation ne semble pas assez explicite sur les niveaux de support, les modeles recommandes, les limites connues et les couts.

**Pourquoi c'est important:** L'experience utilisateur depend fortement du modele choisi. Un mauvais provider peut faire echouer Cortex alors que l'orchestrateur fonctionne correctement.

**Action recommandee:** Ajouter une matrice providers/modeles: qualite attendue par workflow, streaming, tool calling, cout approximatif, local/remote, configuration minimale, limitations connues.

### 6. Observabilite et debogage encore trop orientes developpeur
**Statut:** Terminé
**Preuve:** Couvert par `cortex.run.json`, écrit pour les runs réussis, échoués et interrompus. Le rapport contient timeline, agents, erreurs, fichiers, outils observables, métriques de base et résumé d'échec.

**Constat:** Il existe du verbose logging et des evenements TUI, mais il manque une vue claire pour diagnostiquer pourquoi un run a echoue: provider, prompt, outil, fichier, test, timeout, budget contexte.

**Pourquoi c'est important:** Les workflows multi-agents echouent souvent de maniere partielle. Sans diagnostics exploitables, l'utilisateur ne peut pas corriger le probleme ni fournir un rapport utile.

**Action recommandee:** Ajouter un rapport de run structure: timeline, agents executes, prompts tronques ou non, outils appeles, erreurs, fichiers modifies, commandes lancees, cause probable d'echec.

### 7. Gestion des couts et quotas absente
**Statut:** En cours
**Preuve:** `cortex.run.json` expose les champs `metrics`, `tokens_total` quand disponible et `cost_status`, mais les limites de budget et l'estimation provider précise ne sont pas encore implémentées.

**Constat:** Cortex peut appeler plusieurs agents, workers paralleles, web search et providers distants, mais ne semble pas exposer un budget clair par run.

**Pourquoi c'est important:** Un utilisateur peut declencher des couts eleves sans comprendre combien d'appels ont ete faits ni pourquoi.

**Action recommandee:** Ajouter estimation et suivi: tokens input/output par agent, cout estime par provider, limite de cout par run, alerte avant depassement.

### 8. Custom agents et workflows: validation trop critique pour rester permissive
**Statut:** Terminé
**Preuve:** Couvert par `src/custom_validation.rs`, `cortex validate`, `/validate`, validation pré-exécution des workflows custom, blocage des agents manquants/outils inconnus/YAML invalide, et tests Rust dédiés.

**Constat:** Les workflows custom et agents Markdown rendent Cortex extensible, mais ils introduisent un format declaratif qui peut etre incomplet, contradictoire ou dangereux.

**Pourquoi c'est important:** Une mauvaise definition custom peut produire des erreurs difficiles a comprendre ou contourner les garde-fous attendus.

**Action réalisée:** Validation structurée ajoutée pour les agents et workflows custom: schéma, agents manquants, outils inconnus, YAML invalide, collisions avec workflows intégrés, commande `cortex validate`, commande `/validate`, blocage pré-exécution, et tests dédiés. Les raffinements futurs peuvent couvrir permissions fines, cycles de dépendances, taille de prompts et exemples enrichis.

### 9. Experience de reprise de session a durcir
**Statut:** Terminé
**Preuve:** Couvert par `cortex.checkpoint.json`, qui stocke l'état de reprise du workflow `dev`: phase courante, phases terminées, prochaine action, prompt d'origine, fichiers suivis, hashes SHA-256 et détection de conflits avant reprise.

**Constat:** La reprise apres interruption est une fonctionnalite forte, mais elle depend de l'etat disque, de l'historique de session et de la coherence des fichiers deja generes.

**Pourquoi c'est important:** Reprendre un run dans un etat partiellement modifie peut creer des incoherences ou ecraser du travail utilisateur.

**Action réalisée:** Checkpoints explicites ajoutés avec état de reprise: phase courante, fichiers créés, hash des fichiers, agent responsable, prochaines actions, conflits détectés.

### 10. Documentation d'utilisation avancee incomplete
**Statut:** Terminé
**Preuve:** Couvert par `docs/BETA.md` et les liens ajoutés dans `README.md`.

**Constat:** Le README est riche, mais la densite des features rend l'apprentissage difficile.

**Pourquoi c'est important:** Les nouveaux utilisateurs ont besoin de parcours courts: installer, connecter un provider, lancer un workflow, comprendre les outputs, reparer un echec.

**Action recommandee:** Ajouter des guides par persona: indie hacker, dev local Ollama, equipe qui fait du code review, freelance prospecting.

## Lacunes moyennes

### 11. Manque de politique claire sur les donnees et la confidentialite
**Statut:** Terminé
**Preuve:** Couvert par `docs/PRIVACY.md`, qui documente les données envoyées aux providers, les logs locaux, la gestion des secrets, web search, et les options opt-out.

**Constat:** Le produit met en avant le local et l'absence de lock-in, mais supporte aussi de nombreux providers distants.

**Pourquoi c'est important:** Les utilisateurs doivent savoir quelles donnees partent vers quels services.

**Action recommandee:** Ajouter une page "Data & Privacy": donnees envoyees aux providers, logs locaux, secrets, web search, retention, opt-out.

### 12. Versioning des prompts non formalise
**Statut:** Terminé
**Preuve:** Couvert par `docs/PROMPT_CHANGELOG.md`, qui définit les conventions de versioning, les niveaux de sévérité et le changelog initial.

**Constat:** Les prompts sont au coeur du comportement, mais leur evolution n'est pas traitee comme une surface produit versionnee.

**Pourquoi c'est important:** Les changements de prompts peuvent casser la qualite des workflows sans changement Rust visible.

**Action recommandee:** Ajouter changelog de prompts, tests/evals lies aux prompts, et conventions de revue pour modifications d'agents.

### 13. Pas de strategie claire de compatibilite des sorties generees
**Statut:** Terminé
**Preuve:** `cortex.manifest.json` généré automatiquement dans le répertoire de sortie à chaque run réussi (`src/orchestrator.rs` → `write_manifest()`). Contient version Cortex, workflow, provider, modèles, prompt et commandes de vérification.

**Constat:** Cortex genere des projets dans le repertoire courant, mais il manque une strategie de compatibilite entre versions de Cortex et structures de projet generees.

**Pourquoi c'est important:** Les utilisateurs peuvent vouloir reprendre ou maintenir un projet genere par une ancienne version.

**Action recommandee:** Ecrire un `cortex.manifest.json` dans chaque projet genere avec version Cortex, workflow, provider, modeles, prompts et commandes de verification.

### 14. Release process a renforcer
**Statut:** Terminé
**Preuve:** Couvert par `RELEASE.md`, qui définit la checklist release complète: tests, evals, checksums, smoke tests multi-plateforme, rollback.

**Constat:** Il existe install/update et verification SHA, mais il manque une checklist release visible dans le depot.

**Pourquoi c'est important:** Un outil CLI distribue en binaire doit inspirer confiance, surtout s'il manipule des fichiers et execute des commandes.

**Action recommandee:** Ajouter `RELEASE.md`: tests requis, evals, generation checksums, smoke tests install Linux/macOS/Windows, rollback.

### 15. Tests TUI et UX terminal a completer par scenarios reels
**Statut:** À faire
**Preuve:** Non traité dans ce lot.

**Constat:** Les widgets ont des tests headless, mais les flux clavier longs restent probablement difficiles a couvrir.

**Pourquoi c'est important:** La valeur percue de Cortex passe beaucoup par la TUI. Les bugs d'interruption, popup, resume, diff viewer ou input peuvent ruiner l'experience.

**Action recommandee:** Ajouter des scripts de smoke test interactifs ou snapshots de sessions TUI avec sequences clavier.

## Lacunes produit et go-to-market

### 16. Audience cible trop implicite
**Statut:** Terminé
**Preuve:** Couvert par `docs/BETA.md` (section "Primary Beta Audience" ajoutée: indie devs/solo builders), `docs/COMPARISON.md` (positionnement concurrentiel explicite) et `docs/BETA.md` chemin beta recommandé.

**Constat:** Le PRD liste plusieurs utilisateurs, mais ne choisit pas clairement le premier segment a convaincre.

**Pourquoi c'est important:** Les besoins d'un founder non technique, d'un senior engineer et d'un freelance prospecting sont tres differents.

**Action recommandee:** Choisir un ICP principal pour la beta et adapter README, site, demo et workflows a ce segment.

### 17. Comparaison concurrentielle insuffisante
**Statut:** Terminé
**Preuve:** Couvert par `docs/COMPARISON.md`, qui inclut une matrice de comparaison avec Claude Code, Cursor, Aider, Copilot Workspace et Devin, et précise les cas d'usage de Cortex.

**Constat:** Cortex ressemble par certains aspects a Claude Code, Cursor, OpenCode, Aider, Copilot Workspace et Devin-like tools.

**Pourquoi c'est important:** Sans difference claire, l'utilisateur evaluera Cortex comme "un agent de plus".

**Action recommandee:** Ajouter une section de positionnement: multi-agent workflows, local-first, workflows personnalisables, TUI, generation de depot complet.

### 18. Pas de strategie de support et feedback beta
**Statut:** Terminé
**Preuve:** Couvert par `.github/ISSUE_TEMPLATE/failed_run.md` (runs échoués), `bug_report.md`, `feature_request.md`, `provider_request.md`, `security_report.md` et `quality_report.md`. Tous les canaux de feedback beta sont en place.

**Constat:** Le projet est en beta, mais il manque un canal structure pour rapporter bugs, partager logs et collecter les cas d'usage.

**Pourquoi c'est important:** Une beta utile doit apprendre vite des echecs reels.

**Action recommandee:** Ajouter templates GitHub Issues: bug run, provider issue, generated project quality, feature request, security report.

### 19. Promesse "software company" potentiellement trop forte
**Statut:** Terminé
**Preuve:** Couvert par `docs/BETA.md`, qui recadre la promesse beta et précise les limites du résultat généré.

**Constat:** La metaphore est memorable, mais elle peut creer des attentes de niveau agence complete.

**Pourquoi c'est important:** Si le resultat ressemble a un scaffold avance, la promesse peut sembler excessive.

**Action recommandee:** Recalibrer le wording: "agentic project factory", "multi-agent CLI for project generation", ou garder la formule mais clarifier les limites beta.

## Lacunes techniques transversales

### 20. Tests de securite adversariaux manquants
**Statut:** Terminé
**Preuve:** Tests adversariaux ajoutés pour redaction de secrets, frontières tools (`filesystem`, `terminal`, `email`, `web_search`), validation custom, et updater. Les attaques composées couvrent prompt injection web, définitions custom dangereuses, symlink/traversal, payloads shell-like, email dry-run, et checksums updater suspects.

**Constat:** Les tests couvrent des cas normaux et certains garde-fous, mais pas assez les attaques composees.

**Pourquoi c'est important:** Les agents lisent du contenu non fiable et peuvent appeler des outils.

**Action recommandee:** Ajouter des tests adversariaux: prompt injection dans README externe, URL qui demande de lire `.env`, agent custom demandant `/etc/passwd`, symlink vers hors sandbox, commande shell deguisee.

### 21. Isolation des outputs utilisateur a preciser
**Statut:** Terminé
**Preuve:** L'orchestrateur (`src/orchestrator.rs` → `run_with_project_dir`) émet désormais un avertissement explicite si le répertoire de sortie est non vide avant de démarrer le workflow. Le message conseille d'utiliser `cortex resume` pour continuer un run existant.

**Constat:** Le workflow `dev` ecrit dans le repertoire de lancement. Cela peut etre pratique, mais dangereux si l'utilisateur lance Cortex dans un repo existant.

**Pourquoi c'est important:** Le risque d'ecraser ou melanger des fichiers est eleve.

**Action recommandee:** Par defaut, generer dans un sous-dossier nomme, ou exiger confirmation explicite avant ecriture dans un repertoire non vide.

### 22. Gestion des secrets a renforcer
**Statut:** Terminé
**Preuve:** Redaction centrale dans `src/secrets.rs`, appliquée aux artefacts de run (`cortex.log`, `cortex.manifest.json`), aux previews email et au contexte web search, avec tests de non-régression.

**Constat:** Cortex gere des API keys, SMTP, OAuth et providers distants.

**Pourquoi c'est important:** Les logs, prompts et outputs ne doivent jamais exposer de secrets.

**Action recommandee:** Centraliser le masquage des secrets, ajouter tests de non-regression, scanner les logs avant ecriture et exclure secrets du contexte agent.

### 23. Controle de concurrence et annulation a tester sous charge
**Statut:** À faire
**Preuve:** Non traité dans ce lot.

**Constat:** Le projet utilise tokio, workers paralleles, cancellation tokens et event bus.

**Pourquoi c'est important:** Les bugs de concurrence apparaissent rarement dans les tests simples mais causent des freezes, doublons, pertes d'evenements ou fichiers partiels.

**Action recommandee:** Ajouter tests de stress: interruption pendant tool call, provider lent, worker panique, channel ferme, resume apres cancellation.

### 24. Dependances et supply chain a surveiller
**Statut:** Terminé
**Preuve:** `cargo audit` et `cargo deny` ajoutés comme jobs dans `.github/workflows/ci.yml`. Fichier `deny.toml` ajouté pour la configuration des licences et advisories.

**Constat:** Le projet depend de crates reseau, AWS, SMTP, TUI, parsing YAML/TOML et update binaire.

**Pourquoi c'est important:** La surface supply chain est large pour un outil qui tourne localement sur les machines developpeur.

**Action recommandee:** Ajouter `cargo audit`, `cargo deny`, verification licenses et dependabot/renovate.

## Prochaines etapes recommandees

1. Etendre la matrice d'acceptation des outputs pour le workflow `dev` avec des resultats reels de beta.
2. Completer le harness `evals/` avec scoring, historique de runs et campagnes reproductibles.
3. Maintenir et etendre le modele de menace et la suite adversariale a mesure que les surfaces tools, providers, custom workflows, web search, email et updater evoluent.
4. Ajouter un mode de run avec budget: tokens, cout estime, limites et rapport final.
5. Generer un `cortex.manifest.json` par run pour audit, reprise et debogage.
6. Choisir le workflow phare de la beta publique et marquer les autres comme experimentaux si necessaire.
7. Durcir l'ecriture dans les repertoires non vides avec confirmation ou sous-dossier par defaut.
8. Ajouter templates GitHub Issues et guide "How to report a failed run".
9. Introduire `cargo audit` / `cargo deny` dans la CI.

## Suivi des lots

- 2026-05-18 — Lot docs/process beta terminé: guide beta, guide providers, template failed run, liens README. Lacunes terminées: 4, 5, 10, 19. Lacunes partiellement traitées: 16, 18.
- 2026-05-18 — Lot quality/evals dev terminé: matrice d'acceptation `dev`, fixtures `evals/dev/`, checker minimal pour outputs générés. Lacunes terminées: 1. Lacunes partiellement traitées: 3.
- 2026-05-18 — Lot docs/supply chain/evals/isolation terminé: PRIVACY.md, PROMPT_CHANGELOG.md, RELEASE.md, COMPARISON.md, ICP ajouté dans BETA.md, templates GitHub Issues (security_report, quality_report), cargo audit/deny dans CI (deny.toml), run_campaign.sh + evals/runs/, cortex.manifest.json généré par run, avertissement répertoire non vide. Lacunes terminées: 3, 11, 12, 13, 14, 16, 17, 18, 21, 24.
- 2026-05-19 — Lot sécurité/secrets terminé: modèle de menace, redaction centrale, logs/manifests/email/web search redacted, premiers tests adversariaux et durcissement symlink filesystem. Lacunes terminées: 22. Lacunes partiellement traitées: 2, 20.
- 2026-05-19 — Lot validation custom terminé: validation structurée agents/workflows custom, commandes `cortex validate` et `/validate`, blocage pré-exécution des workflows invalides. Lacune terminée: 8.
- 2026-05-20 — Lot observabilité complète terminé: `cortex.run.json` généré pour succès/échec/interruption, timeline structurée, résumés agents, fichiers, outils observables, métriques de base, redaction secrets et documentation de partage. Lacune terminée: 6. Lacune partiellement traitée: 7.
- 2026-05-20 — Lot reprise robuste terminé: `cortex.checkpoint.json`, reprise structurée du workflow `dev`, validation des hashes, refus des reprises ambiguës et documentation des artefacts. Lacune terminée: 9.
- 2026-05-21 — Lot sécurité adversariale avancée terminé: labellisation web search non fiable, tests d'attaques composées custom/tools/email/updater, et modèle de menace mis à jour. Lacunes terminées: 2, 20.
