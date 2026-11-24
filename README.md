# Pantheon Battler

Auto battler joué sur le forum de MyHordes : le MJ poste la boutique et les combats, les
joueurs répondent avec leurs ordres. Ce projet contient le moteur de règles et un simulateur
d'équilibrage avec des bots.

## Utilisation

```sh
cargo build --release
./target/release/pantheon-battler roster              # tableau des unités
./target/release/pantheon-battler sample --seed 3     # une partie avec son journal complet
./target/release/pantheon-battler sample --forum      # les combats au format forum (Twino), prêts à coller
./target/release/pantheon-battler duels --tier 1      # duels 1 contre 1 au sein d'un rang
./target/release/pantheon-battler games -n 20000      # rapport d'équilibrage
./target/release/pantheon-battler games --fixed Croissance-moyen --count 3   # 3 joueurs sur la même stratégie
./target/release/pantheon-battler --data autre/ games # tester une variante des données
./target/release/pantheon-battler arena -n 2000       # simulation du mode Arène
./target/release/pantheon-battler fight --a-file A.txt --b-file B.txt --dice 12,3,77,45,9,100 --forum
                                                      # un défi de l'Arène, comme la page web
./target/release/pantheon-battler shop --level 1 --level 2 --players 8 --page 12
                                                      # les boutiques d'une page du Bazar
                                                      # (--stock reserve.json : exemplaires restants,
                                                      #  --seed N : refaire un tirage)
```

Les réglages d'équilibrage sont dans `data/units.toml` et `data/rules.toml` : pas besoin de
recompiler (sauf pour la page web, qui embarque les données).

## Page de combat (mode Arène)

En ligne : <https://pantheon-battler.chaton.ninja/> (GitHub Pages : à chaque push sur
`main` qui touche `web/`, `.github/workflows/pages.yml` copie `web/` dans la branche `gh-pages`,
que Pages sert telle quelle).

`web/` est un site statique en français, sans dépendance : onglets **Combat** (rejoue un défi à
partir des deux compositions et des six dés, donne le texte pour le forum, le bilan et les
compositions mises à jour ; les créatures se glissent dans les 4 emplacements de chaque joueur)
et **Aide**. Le code commun (moteur, aperçu du forum, infobulles) est dans `web/core.js`.

`web/gm_tool/` contient les outils des MJ, sans lien depuis la page des joueurs : tirage des
boutiques du Bazar, réserve commune et journal des actions. La réserve est partagée entre MJ par
un Gist secret de GitHub : chaque navigateur écrit son propre fichier `journal-<appareil>.json`
(rien ne s'écrase), et la réserve est recalculée en rejouant les journaux de tous les MJ.

La page fait tourner le vrai moteur
Rust compilé en WebAssembly : pour les mêmes entrées, la page et `pantheon-battler fight`
donnent exactement le même texte.

- `src/compo.rs` : format des compositions (lecture tolérante, erreurs et avertissements).
- `src/challenge.rs` : seed (Σ (dé − 1) × 100^(5 − i), attaquant d'abord), combat, bilan
  (🏆, ✨, séries, Rancune, Offrande, Pillage…) et texte du forum.
- `src/bazar.rs` : tirage des boutiques du Bazar (rang selon le niveau, unité en proportion
  des exemplaires restants) et message du forum ; le simulateur `arena` utilise le même tirage.
- `src/api.rs` : API JSON (`info`, `parse`, `format`, `fight`, `shop`) commune à la page
  (`wasm/`, exports bruts `pb_alloc`, `pb_call`, `pb_free`) et à la commande cachée
  `pantheon-battler api` (une requête JSON par ligne).

Construire après chaque changement du moteur ou des données (les fichiers construits sont
versionnés, l'hébergeur n'a rien à compiler) :

```sh
rustup target add wasm32-unknown-unknown   # une seule fois
scripts/build-web.sh                       # ou : pwsh scripts/build-web.ps1
```

Le script compile `web/engine.wasm`, l'intègre en base64 dans `web/engine-wasm.js` (la page
marche donc aussi depuis `file://`), puis lance `scripts/verify-web.mjs` : chaque défi de
référence de `tests/defis/<cas>/` (`a.txt`, `b.txt`, `dice.txt`, `expected.txt` produit par
`fight --forum`) doit donner exactement `expected.txt`, et 2 000 défis tirés au hasard doivent
donner la même réponse par la page et par la CLI, octet pour octet.

Tester en local : ouvrir `web/index.html`, ou `python -m http.server -d web 8000` puis
<http://localhost:8000/>.

Autre serveur : copier `deploy.env.example` en `deploy.env` (jamais versionné), le remplir, puis
`scripts/deploy-web.sh` (reconstruit, vérifie, demande confirmation et envoie `index.html`,
`app.js`, `style.css` et `engine-wasm.js`). `--dry-run` montre ce qui serait envoyé. La page
n'a besoin d'aucun réglage serveur ; HTTPS est conseillé pour les boutons « Copier ».

## Vocabulaire

- **Faveurs** 🙏 : la monnaie. **Essence divine** ✨ : les vies des joueurs. **PV** ❤ : les
  points de vie d'une unité.
- **Archétypes** : 🌱 Croissance · ♻️ Cycle · 🎲 Chance · 🔒 Contrôle · 🐺 Revanche.
- **Ordres** : `acheter`, `vendre`, `relancer`, `investir N`, `aligner` /
  `réserver`.

## Règles telles qu'implémentées

- Dégâts = max(ATQ − DÉF, `min_damage`). La DÉF s'applique aux attaques comme aux dégâts
  des capacités.
- Les PV sont entièrement restaurés au début de chaque combat ; seuls les bonus permanents
  sont conservés.
- Les unités sont alignées de gauche à droite ; la position compte.
- Début de combat : Dévoration → Bonne étoile → Phalange → Pétrification → Emprise
  (chacune simultanée). La Phalange est recalculée pendant le combat, après chaque Gel et
  chaque attaque.
- Puis les camps attaquent à tour de rôle, en commençant par celui qui a le plus d'unités
  (tirage au sort en cas d'égalité). À son tour, un camp attaque avec son unité la plus à
  gauche qui n'a pas encore attaqué, puis la suivante à sa droite, et ainsi de suite ; une
  fois que toutes ont attaqué, on recommence par la gauche. L'attaquant frappe un ennemi
  au hasard, qui riposte en même temps. Au-delà de `max_fight_steps` attaques, c'est un
  match nul.
- Une attaque : Gel de l'attaquant → coups simultanés (Regard de pierre) → Néant et
  Châtiment des unités tuées par ces coups → Représailles et Effroi → Fascination et Chaos →
  morts. Une unité anéantie (Néant) quitte la composition de son joueur et retourne dans la
  réserve ; Héritage se déclenche à chaque mort, même suivie d'un retour.
  Une Pétrification annule la prochaine attaque du camp visé (son tour est perdu).
- Une unité dévorée est consommée sans déclencher ses effets de mort, et seulement pour ce
  combat.
- Renaissance ramène l'unité une fois par combat avec 1 PV.
- Réserve commune (`pool_per_player`, proportionnelle au nombre de joueurs, au moins
  `merge_copies`) : les offres sont tirées parmi les exemplaires restants, pondérées par leur
  nombre, et les offres non achetées y retournent en fin de phase de boutique. Les unités
  vendues, disparues, transformées ou éliminées y retournent aussi. Les récompenses de fusion
  et les copies du Changelin en sortent. Les joueurs font leurs achats l'un après l'autre,
  dans un ordre tiré au sort à chaque manche.
- Boutique : les faveurs sont investies peu à peu ; atteindre un seuil fait monter le niveau
  dès la manche suivante, et l'excédent est reporté. Seuils : `upgrade_threshold` (4, 8 puis 8 faveurs).
- Réserve (`reserve_size`) : unités gardées qui ne combattent pas, ne déclenchent pas leurs
  mots-clés, ne comptent pas pour les synergies et ne donnent pas accès aux ordres. Une unité
  achetée va sur le terrain s'il y a de la place, sinon en réserve ; réorganiser est gratuit.
  Les fusions comptent les exemplaires des deux côtés. Vendre depuis la réserve déclenche
  quand même les effets « à la vente ».
- Fusion : acheter le 3e exemplaire identique les fusionne en une unité ⭐ : stats de base et
  effets des mots-clés multipliés par `merge_power` de son rang (par ex. ×2 : Renaissance deux
  fois, deux attaques ennemies annulées par la Pétrification), plus le meilleur bonus
  permanent des exemplaires. Le joueur reçoit aussi une unité aléatoire du rang suivant.
- Ascension (`top_merge_wins`, activé) : fusionner trois unités de rang 3 fait gagner la
  partie à la fin de cette phase de boutique ; pas de combats cette manche-là, les autres
  joueurs sont classés selon leur situation.
- Fin de boutique : les Changelins agissent d'abord (une copie peut compléter une fusion),
  puis Vigueur et Bénédiction.
- Magot supprime le plafond de faveurs et rapporte
  des intérêts.
- Synergies (`[[synergy]]` dans rules.toml) : aligner assez d'unités d'un archétype donne un
  mot-clé au joueur. Actuellement : 3 unités Revanche → Endurance (−2✨ par défaite ou nul, min 1).
- Essence divine : 25✨ au départ. Une défaite coûte 1✨ par rang de chaque unité adverse encore
  debout (une unité ⭐ compte double) ; un match nul coûte à chaque joueur le numéro de la manche.
- À chaque manche, les joueurs encore en vie sont appariés au hasard ; avec un nombre impair,
  l'un d'eux affronte une copie fantôme du terrain d'un autre (seul le vrai joueur est
  concerné).
- Classement final : les survivants selon leur essence divine (✨) puis leurs victoires, puis
  les éliminés, du dernier au premier.

## Bots

24 stratégies = archétype favori (aucun, « Générique », ou l'un des 5) × plan de boutique
(bas : n'investit jamais, moyen : investit ses restes jusqu'au niveau 3, haut : investit ses
restes jusqu'au niveau 4, ruée : investit avant d'acheter, jusqu'au niveau 4). Pendant la première moitié de la partie, les bots Revanche
n'achètent que des unités Revanche une fois qu'ils en alignent deux.
Les bots sont volontairement simples : ils repèrent les gros déséquilibres, pas les subtils.
