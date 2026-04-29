# Plan de correction: Affichage des derniers agents

## Objectif
Corriger le problème d'affichage du panel d'agents (TUI) afin que, lorsqu'il y a plus de 6 agents lancés, l'interface affiche les 6 agents les plus récents au lieu des 6 premiers.

## Fichiers clés & Contexte
- `src/tui/widgets/agent_panel.rs` : Gère le rendu de la grille des agents actifs. Le code actuel prend `self.agents.len().min(6)` et itère sur les 6 premiers éléments du tableau.

## Étapes d'implémentation
1. **Modifier `src/tui/widgets/agent_panel.rs`** :
   - Dans le rendu "Grid Mode" (vers la ligne 131), créer une variable `visible_agents` qui contient une *slice* des 6 derniers agents du tableau `self.agents` s'il y en a plus de 6.
   - Mettre à jour la logique de la grille pour utiliser `visible_agents` au lieu de `self.agents`.

## Code prévu (Aperçu)
```rust
        // ── Grid Mode ────────────────────────────────────────────────────────
        // Obtenir au maximum les 6 derniers agents
        let visible_agents = if self.agents.len() > 6 {
            &self.agents[self.agents.len() - 6..]
        } else {
            self.agents
        };

        let count = visible_agents.len();
        let (rows, cols) = match count {
            // ... (logique inchangée)
        };
        // ... (logique inchangée)
            for c in 0..cols {
                let index = r * cols + c;
                if index < count {
                    render_agent_block(frame, &visible_agents[index], col_rects[c]);
                }
            }
```

## Vérification & Tests
- Compiler le projet avec `cargo build`
- Lancer un workflow impliquant plus de 6 agents et vérifier visuellement que les agents affichés défilent pour toujours montrer les 6 derniers créés.
