# Plan d'implémentation : Rendre la grille des agents responsive

## Objectif
Résoudre le problème de lisibilité des réponses des agents sur les petits écrans en rendant le panneau des agents (`AgentPanelWidget`) responsive. La disposition en grille s'adaptera automatiquement en limitant le nombre de colonnes si l'écran est trop étroit.

## Fichiers Modifiés
- `src/tui/widgets/agent_panel.rs`

## Étapes d'Implémentation
1. **Modifier la logique de disposition en grille** dans la méthode `render` de `AgentPanelWidget`.
2. Calculer la largeur disponible (`inner.width`) et définir une largeur minimale souhaitable pour une colonne d'agent (ex: 35 ou 40 caractères).
3. Déterminer le nombre maximum de colonnes qui peuvent tenir confortablement : `max_cols = (inner.width / min_col_width).max(1)`.
4. Restreindre les colonnes désirées par ce `max_cols`.
5. Calculer le nombre de lignes dynamiquement : `rows = (count + cols - 1) / cols`.

## Exemple de Logique Ciblée
```rust
// Division de la zone interne basée sur la taille disponible et le nombre d'agents
let count = visible_agents.len();

// Assurer une largeur minimale de 35 caractères par colonne
let min_col_width = 35;
let available_width = inner.width as usize;
let max_cols = (available_width / min_col_width).max(1);

let desired_cols = match count {
    1 => 1,
    2 | 3 | 4 => 2,
    _ => 3,
};

let cols = desired_cols.min(max_cols);
let rows = (count + cols - 1) / cols;
```

## Vérification et Tests
- Lancer le TUI (`cargo run`) dans un terminal large pour vérifier que l'affichage reste sur plusieurs colonnes pour plusieurs agents.
- Redimensionner la fenêtre du terminal pour la rendre étroite et vérifier que la grille bascule vers une disposition à 2 colonnes ou 1 seule colonne.
- Confirmer que le texte à l'intérieur des colonnes reste correctement enveloppé (wrap) et lisible sans déborder sur les bordures.