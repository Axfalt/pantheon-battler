# Serveur du Bazar

Le serveur ordonne les achats du Bazar : un joueur **confirme** son message dans l'onglet Bazar du
site avant de le poster, l'unité n'est alors plus à vendre pour les autres ; il peut **annuler**
un message confirmé. Il garde aussi le **dieu de chaque joueur** (faveurs, niveau, terrain,
réserve, derniers messages) dans un compte. Les outils MJ y publient la boutique en cours et la
réserve commune, et importent les messages confirmés dans leur journal.

| Fichier | Rôle |
|---|---|
| `handler.mjs` | Les routes et les règles (commun au serveur local et à AWS) |
| `store-memory.mjs` | Stockage en mémoire : serveur local, tests |
| `store-dynamo.mjs` | Stockage DynamoDB (transactions conditionnelles : un achat par emplacement, jamais sous 0 exemplaire, un nom par compte) |
| `myhordes.mjs` | Vérification d'un joueur par sa clé MyHordes (et une fausse MyHordes pour le serveur local) |
| `lambda.mjs` | Point d'entrée AWS Lambda (adresse de fonction, *Function URL*) |
| `terraform/` | Déploiement AWS avec Terraform : table, fonction, adresse, alerte de dépense |
| `dev.mjs` | Serveur local : le site et l'API sur un seul port |
| `handler.test.mjs` | Tests (`node --test server/handler.test.mjs`) |

## Les comptes des joueurs

- **Sans clé MyHordes** : le compte se crée au premier message confirmé, avec le nom tapé ; ce nom
  est alors à ce compte. Le navigateur garde une session (un jeton dans son stockage local : un
  vrai cookie ne marcherait pas, le serveur étant sur un autre domaine que le site). Le bouton
  « Code pour un autre appareil » donne un code qui ouvre le même dieu ailleurs.
- **Avec la clé MyHordes** (facultative) : le serveur demande à MyHordes qui est le joueur
  (`https://myhordes.eu/api/x/json/me`) ; le nom est donc prouvé, et la même clé rouvre le dieu
  sur n'importe quel appareil. La clé sert à cette seule demande : elle n'est ni gardée ni
  journalisée. Un nom vérifié par MyHordes reprend ce nom à un compte sans clé. Un joueur déjà
  connecté sans clé qui donne sa clé garde son dieu, désormais vérifié.
- Il faut pour cela une **clé d'application MyHordes**, à demander à l'équipe MyHordes ; sans
  elle, la connexion MyHordes est simplement cachée et tout le monde joue sans clé.
- L'onglet **Compte** du site montre le compte : nom, dates, code pour un autre appareil,
  déconnexion des autres appareils, le dieu (modifiable, avec sa composition de défi à copier
  pour le forum : 🏆, ✨ et séries sont aussi gardés dans le compte), et l'historique des
  30 derniers messages (leur texte contient l'état du dieu juste après). Ces messages sont un
  élément à part du compte, écrit seulement avec un message ou une annulation : enregistrer le
  dieu reste une petite écriture.
- Chaque enregistrement du dieu porte un numéro de version : deux appareils ne peuvent pas
  s'écraser (le second est rechargé avec un message).
- Annuler un message remet le dieu comme avant ce message s'il n'a pas changé depuis ; sinon le
  dieu reste tel quel (le joueur corrige à la main). Un MJ qui annule un message remet de même le
  dieu du joueur.
- **Tout est effacé 60 jours après sa dernière modification** (la durée maximale d'une partie) :
  comptes, sessions, noms, messages, boutique.

## Essayer en local

```sh
node server/dev.mjs           # données gardées dans bazar-dev.json entre deux lancements
node server/dev.mjs --fresh   # repartir d'un Bazar vide
```

- Joueurs : <http://localhost:8787/>, onglet **Bazar** (deux fenêtres de navigation privée = deux joueurs).
- Connexion MyHordes **simulée** en local : la clé `test-Alice` est la joueuse Alice (aucune
  demande n'est faite à MyHordes). `--mh-appkey <clé>` utilise la vraie MyHordes.
- MJ : <http://localhost:8787/gm_tool/>. En local, l'outil MJ se connecte tout seul au serveur
  (`http://localhost:8787/api`, clé `dev`) : créez la réserve (onglet Réserve), tirez une boutique
  et « Copier et enregistrer » ; elle apparaît chez les joueurs. Si l'outil MJ est ouvert depuis
  une autre adresse (autre aperçu local, 127.0.0.1…), entrez cette adresse et cette clé dans
  l'onglet **Partage** → **Serveur du Bazar**.

Après `--fresh`, le serveur reprend au message n° 1 et tous les noms sont libres : l'outil MJ le
signale et n'importe plus ses messages tant que son journal contient ceux de l'ancien serveur
(repartir d'un journal vide, par exemple dans une fenêtre de navigation privée).

`web/config.js` ne pointe vers le serveur local que si la page est ouverte depuis `localhost` :
sur le site public, tant qu'aucun serveur n'est déployé, l'onglet Bazar prépare les messages mais
ne peut pas les confirmer.

## Routes

| Route | Qui | Rôle |
|---|---|---|
| `GET /bazar[?since=N]` | tous | boutique, emplacements achetés, exemplaires disponibles, derniers messages (avec `since` : seulement ceux après le n° N) |
| `GET /events?after=N` | tous | messages et annulations après le n° N |
| `POST /accounts` | nouveau joueur | crée un compte sans MyHordes `{ name, state }` → jeton de session |
| `POST /login/myhordes` | joueur | `{ key, state }` → jeton ; relie le compte en cours s'il n'est pas encore vérifié |
| `GET /me`, `PUT /me` | joueur (`Authorization: Bearer s.…`) | le dieu et les derniers messages ; l'enregistrer `{ version, state?, name? }` |
| `POST /sessions`, `DELETE /sessions` | joueur | un code pour un autre appareil ; se déconnecter |
| `DELETE /sessions/others` | joueur | déconnecte tous ses autres appareils (un code donné par erreur) |
| `POST /posts` | joueur | confirme un message (le dieu du compte est mis à jour dans la même transaction) |
| `POST /posts/<n>/cancel` | son joueur, ou un MJ | annule un message |
| `PUT /shop`, `PUT /stock`, `GET /gm` | MJ (`Authorization: Bearer <clé MJ>`) | boutique, réserve, test de la clé et ce que le serveur a déjà |

## Déployer sur AWS (plus tard), avec Terraform

Tout est prévu pour rester dans l'offre gratuite **permanente** d'AWS (pas celle des 12 premiers
mois) :

| Service | Offert chaque mois, sans limite de durée | Usage prévu |
|---|---|---|
| Lambda (le code du serveur) | 1 million d'appels, 400 000 Go-secondes | ~100 000 appels |
| Adresse de la fonction (Function URL) | rien de plus que Lambda | — |
| DynamoDB (les données) | 25 Go, 25 unités de lecture et 25 d'écriture | quelques Mo, 15 + 10 unités |
| CloudWatch Logs (journaux techniques) | 5 Go | quelques dizaines de Mo, gardés 14 jours |

- La table a une capacité fixe (15 lectures, 10 écritures par seconde) au lieu d'être facturée à
  la requête : en cas d'affluence, des requêtes sont refusées (« réessayez ») au lieu d'être
  facturées. Un onglet Bazar ouvert ne demande que les nouveautés (`?since=`) : environ 4 unités
  de lecture par interrogation, soit une cinquantaine d'onglets ouverts en même temps.
- L'effacement automatique (TTL de DynamoDB) est gratuit.
- Pas d'API Gateway (gratuit seulement 12 mois) : la fonction a sa propre adresse.
- La sauvegarde continue de la table (point-in-time recovery) coûte environ 0,20 $ par Go et par
  mois : pour quelques Mo, moins d'un centime.
- Donnez une adresse e-mail (`alert_email`) : AWS vous écrit dès le premier centime dépensé.

Il faut un compte AWS, [Terraform](https://developer.hashicorp.com/terraform/install) (1.6 ou
plus) et des identifiants AWS (par exemple `aws configure` avec l'[AWS CLI](https://aws.amazon.com/cli/)).

```sh
cd server/terraform
cp terraform.tfvars.example terraform.tfvars   # puis remplir : clé MJ, e-mail, plus tard la clé MyHordes
terraform init
terraform plan      # vérifier ce qui sera créé
terraform apply
```

Ensuite :

1. Copier l'adresse `api_url` affichée à la fin (`https://….lambda-url.<région>.on.aws/`) dans
   `web/config.js` (à la place de `""`).
2. Dans l'outil MJ, onglet **Partage** → **Serveur du Bazar** : cette adresse et la clé MJ.
3. Publier le site (push sur `main`).
4. Si vous avez donné un e-mail : confirmer l'abonnement à l'alerte dans le message d'AWS.

Une mise à jour du code du serveur (fichiers `*.mjs`) se déploie avec `terraform apply`.

- **`terraform.tfvars` et l'état Terraform (`terraform.tfstate`) contiennent la clé MJ** : ils
  sont ignorés par git, gardez-les pour vous (et sauvegardez l'état : sans lui, Terraform ne
  sait plus gérer ce qu'il a créé).
- `allowed_origin` (par défaut `https://pantheon-battler.chaton.ninja`) est la seule adresse
  autorisée à appeler le serveur depuis un navigateur.
- `max_concurrency` : laisser `0` (un compte neuf est déjà limité à 10 exécutions simultanées).
