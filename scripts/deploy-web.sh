#!/usr/bin/env bash
# Publie la page de combat (web/) sur le serveur public.
#
# Réglages : deploy.env à la racine du projet (non versionné ; modèle : deploy.env.example),
# ou le fichier donné par DEPLOY_ENV.
#
# Usage : scripts/deploy-web.sh [--dry-run] [--yes]
#   --dry-run  affiche ce qui serait envoyé, sans construire ni rien envoyer
#   --yes      n'attend pas de confirmation avant l'envoi
set -euo pipefail
cd "$(dirname "$0")/.."

dry_run=0
yes=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    --yes) yes=1 ;;
    *) echo "erreur : option inconnue $arg (options : --dry-run, --yes)" >&2; exit 1 ;;
  esac
done

env_file="${DEPLOY_ENV:-deploy.env}"
if [ ! -f "$env_file" ]; then
  echo "erreur : $env_file absent. Copiez deploy.env.example en deploy.env et remplissez-le." >&2
  exit 1
fi
set -a
# shellcheck disable=SC1090
. "$env_file"
set +a

: "${DEPLOY_METHOD:?DEPLOY_METHOD manquant dans $env_file (rsync, scp, sftp ou ftp)}"
: "${DEPLOY_TARGET:?DEPLOY_TARGET manquant dans $env_file}"
: "${PUBLIC_URL:?PUBLIC_URL manquant dans $env_file}"

# Only what the page needs: the wasm is embedded in engine-wasm.js.
files=(index.html app.js style.css engine-wasm.js)

if [ "$dry_run" = 0 ]; then
  echo "== Construction et vérification (page = CLI)"
  scripts/build-web.sh
fi
for f in "${files[@]}"; do
  [ -f "web/$f" ] || { echo "erreur : web/$f absent, lancez scripts/build-web.sh" >&2; exit 1; }
done

port_ssh=()
port_scp=()
if [ -n "${DEPLOY_PORT:-}" ]; then
  port_ssh=(-e "ssh -p $DEPLOY_PORT")
  port_scp=(-P "$DEPLOY_PORT")
fi
paths=("${files[@]/#/web/}")

case "$DEPLOY_METHOD" in
  rsync) cmd=(rsync -av --chmod=F644 "${port_ssh[@]}" "${paths[@]}" "$DEPLOY_TARGET") ;;
  scp) cmd=(scp "${port_scp[@]}" "${paths[@]}" "$DEPLOY_TARGET") ;;
  sftp)
    host="${DEPLOY_TARGET%%:*}"
    dir="${DEPLOY_TARGET#*:}"
    batch=$(printf 'cd "%s"\n' "$dir"; printf 'put "%s"\n' "${paths[@]}")
    cmd=(sftp "${port_scp[@]}" -b - "$host")
    ;;
  ftp)
    # DEPLOY_TARGET=ftp://hôte/dossier ; FTP_USER et FTP_PASSWORD dans deploy.env.
    : "${FTP_USER:?FTP_USER manquant dans $env_file}"
    rest="${DEPLOY_TARGET#ftp://}"
    host="${rest%%/*}"
    dir="/${rest#*/}"
    cmd=(lftp -p "${DEPLOY_PORT:-21}" -u "$FTP_USER,${FTP_PASSWORD:-}" -e "set ftp:ssl-allow yes; cd \"$dir\"; mput ${paths[*]}; bye" "$host")
    ;;
  *) echo "erreur : DEPLOY_METHOD=$DEPLOY_METHOD inconnu (rsync, scp, sftp ou ftp)" >&2; exit 1 ;;
esac

echo "== Envoi"
echo "Méthode : $DEPLOY_METHOD · cible : $DEPLOY_TARGET"
echo "Fichiers : ${files[*]}"
if [ "$dry_run" = 1 ]; then
  if [ "$DEPLOY_METHOD" = ftp ]; then
    echo "Commande : lftp -p ${DEPLOY_PORT:-21} -u $FTP_USER,*** … $host (cd $dir ; mput ${paths[*]})"
  else
    echo "Commande : ${cmd[*]}"
  fi
  [ "$DEPLOY_METHOD" = sftp ] && printf 'Lot sftp :\n%s\n' "$batch"
  echo "(essai à blanc : rien n'a été envoyé)"
  exit 0
fi
if [ "$yes" = 0 ]; then
  read -r -p "Envoyer ces fichiers ? [o/N] " answer
  case "$answer" in
    o|O|oui|Oui) ;;
    *) echo "Annulé : rien n'a été envoyé."; exit 1 ;;
  esac
fi
if [ "$DEPLOY_METHOD" = sftp ]; then
  printf '%s\n' "$batch" | "${cmd[@]}"
else
  "${cmd[@]}"
fi

echo "== En ligne : $PUBLIC_URL"
echo "Vérifiez que le pied de page montre les mêmes données que 'pantheon-battler fight', puis rejouez un défi de tests/defis/."
