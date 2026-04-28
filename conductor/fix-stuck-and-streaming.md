# Plan : Déblocage et Flux en Temps Réel (Streaming)

Ce plan vise à résoudre le problème de blocage constaté par l'utilisateur (probablement dû à des providers manquants) et à implémenter le flux en temps réel de l'activité des agents.

## Objectifs
1.  **Restaurer les Providers** : Réinstaller `openrouter.rs`, `groq.rs` et `together.rs` qui ont été accidentellement supprimés.
2.  **Implémenter le Streaming** : Modifier les providers pour supporter l'envoi de `TokenChunk` pendant la génération.
3.  **Dégeler l'Interface** : S'assurer que les agents utilisent le streaming pour que l'utilisateur voie l'activité immédiatement.

## Fichiers clés
- `src/providers/mod.rs` : Point d'entrée des LLM.
- `src/providers/openrouter.rs`, `groq.rs`, `together.rs` : Implémentations spécifiques.
- `src/workflows/dev/agents/ceo.rs` (et autres agents) : Utilisation du streaming.

## Étapes d'implémentation

### 1. Restauration des fichiers de Provider
- Créer `src/providers/openrouter.rs` avec le code récupéré du log git.
- Créer `src/providers/groq.rs` avec le code récupéré du log git.
- Créer `src/providers/together.rs` avec le code récupéré du log git.

### 2. Ajout du support Streaming dans `providers/mod.rs`
- Exposer les nouveaux modules dans `mod.rs`.
- Ajouter une fonction `complete_stream(model_str, preamble, prompt, tx, agent_name)` :
    - Elle doit router vers le bon provider.
    - Elle doit utiliser l'API de streaming de `rig`.
    - Pour chaque chunk reçu, elle doit envoyer un `TuiEvent::TokenChunk` via `tx`.

### 3. Mise à jour des Agents (Dev Workflow)
- Modifier `ceo.rs`, `pm.rs`, `tech_lead.rs`, `developer.rs`, `qa.rs`, `devops.rs` pour appeler `complete_stream`.
- Passer l'option `tx` (disponible dans `RunOptions`) à ces appels.

### 4. Vérification
- Lancer un workflow avec OpenRouter.
- Vérifier que les tokens s'affichent au fur et à mesure dans le panneau de l'agent (le flux ajouté précédemment prendra alors tout son sens).

## Note sur le blocage
Le blocage actuel est quasi certainement dû au fait que l'application tente de se connecter à un Ollama local (fallback par défaut) car OpenRouter n'est pas implémenté dans le `mod.rs` actuel. La restauration des providers corrigera cela.
