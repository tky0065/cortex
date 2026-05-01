# Plan d'Implémentation : Généralisation du Système de Tâches

## Objectif
Permettre à n'importe quel agent (et à l'Assistant général) de créer et maintenir un fichier `TASKS.md` pour des requêtes hors-workflow complexes.

## Changements Prévus
1. **Mise à jour du `PREAMBLE` de l'Assistant (`src/assistant.rs`)** :
   - Ajouter une règle stricte : pour toute tâche complexe, l'Assistant doit d'abord créer un fichier `TASKS.md` contenant la liste des étapes (avec `- [ ]`), puis le mettre à jour au fur et à mesure (`- [x]`).
   
2. **Mise à jour des Agents Autonomes (`src/assistant.rs` > `agent_preamble_for_role`)** :
   - Lorsqu'un agent est appelé directement de manière autonome (ex: `/agent devops "déploie mon app"`), le prompt de secours (fallback) intégrera également cette instruction de suivi des tâches.

## Vérification
L'interface utilisateur que nous venons d'implémenter (le widget "Tasks") affichera automatiquement ces tâches, qu'elles proviennent d'un workflow structuré (`/start`) ou d'une demande ponctuelle complexe à l'Assistant.