# Plan d'Implémentation : Gestionnaire de Tâches et Mise à Jour UI

## Objectif
Implémenter un système de gestion de tâches pour suivre l'avancement des workflows complexes, et mettre à jour la disposition de l'interface utilisateur (TUI) pour afficher cette liste de tâches au-dessus des logs. Les logs seront réduits en taille pour laisser plus de place aux informations importantes.

## Choix Techniques
- **Source des tâches** : La liste des tâches sera lue à partir d'un fichier `TASKS.md` (ou similaire) dans le répertoire du projet. Les agents mettront à jour ce fichier, et le TUI reflétera ces changements.
- **Disposition UI** : L'espace droit de l'interface sera divisé verticalement. La liste des tâches (en haut) prendra l'espace dynamique nécessaire (jusqu'à un certain maximum pour éviter de faire disparaître totalement les logs), et les logs (en bas) prendront l'espace restant.
- **Police des logs** : *Note importante* : Dans une interface terminal (TUI) avec `ratatui`, la taille de la police est contrôlée par le terminal de l'utilisateur, pas par l'application. Pour rendre les logs moins envahissants, nous réduirons l'espace qu'ils occupent et nous pourrons ajuster leur style (couleurs plus discrètes si nécessaire).

## Étapes d'Implémentation

### 1. Nouveaux Événements TUI (`src/tui/events.rs`)
- Ajouter une structure de données `Task` (ex: `struct Task { description: String, is_done: bool }`).
- Ajouter un événement `TuiEvent::TasksUpdated { tasks: Vec<Task> }`.

### 2. Widget de Tâches (`src/tui/widgets/tasks.rs`)
- Créer un nouveau widget `TasksWidget`.
- Ce widget rendra une liste de tâches avec des cases à cocher (`[ ]` et `[x]`) et une barre de progression ou un résumé visuel de l'avancement.

### 3. Mise à jour du Layout (`src/tui/layout.rs`)
- Modifier `AppLayout` pour inclure `pub tasks: Rect`.
- Dans `compute()`, diviser l'ancienne zone `logs` en deux : `tasks` (en haut) et `logs` (en bas).
- Utiliser un `Constraint::Length` dynamique pour la zone des tâches, basé sur le nombre de tâches (avec un `Constraint::Max` pour s'assurer que les logs restent visibles), et un `Constraint::Min(0)` pour les logs.

### 4. Mise à jour de l'État de l'Application (`src/tui/mod.rs`)
- Ajouter `tasks: Vec<Task>` à la structure `App`.
- Gérer l'événement `TasksUpdated` dans `on_orchestrator_event` pour mettre à jour `app.tasks`.
- Mettre à jour `App::draw` pour rendre le `TasksWidget` dans la zone `layout.tasks`.

### 5. Surveillance de `TASKS.md`
- Implémenter une logique (probablement dans l'orchestrateur ou via un watcher de fichier de fond) qui lit le fichier `TASKS.md` dans le répertoire de travail actuel (`session.directory`).
- Analyser les lignes du type `- [ ] Tâche 1` ou `- [x] Tâche 2`.
- Émettre l'événement `TasksUpdated` chaque fois que le fichier est modifié.

## Validation
- Lancer un workflow de test qui crée et modifie un fichier `TASKS.md`.
- Vérifier que la liste s'affiche correctement en haut à droite.
- Vérifier que les logs s'affichent correctement en dessous, en prenant l'espace restant.
- Vérifier que les cases à cocher se mettent à jour automatiquement.