variable "region" {
  description = "Région AWS (eu-west-3 = Paris)."
  type        = string
  default     = "eu-west-3"
}

variable "name" {
  description = "Préfixe des ressources (table, fonction, rôle)."
  type        = string
  default     = "pantheon-bazar"
}

variable "gm_key" {
  description = "Clé secrète des MJ (au moins 16 caractères), à entrer dans l'outil MJ, onglet Partage."
  type        = string
  sensitive   = true
  validation {
    condition     = length(var.gm_key) >= 16
    error_message = "La clé MJ doit faire au moins 16 caractères."
  }
}

variable "myhordes_app_key" {
  description = "Clé d'application MyHordes (donnée par l'équipe MyHordes). Vide : pas de connexion MyHordes, les joueurs jouent sans clé."
  type        = string
  sensitive   = true
  default     = ""
}

variable "allowed_origin" {
  description = "Adresse du site autorisée à appeler le serveur depuis un navigateur."
  type        = string
  default     = "https://pantheon-battler.chaton.ninja"
}

variable "ttl_days" {
  description = "Jours après lesquels une donnée non modifiée est effacée (la durée maximale d'une partie)."
  type        = number
  default     = 60
}

variable "alert_email" {
  description = "Adresse prévenue dès que le compte AWS dépense quelque chose dans le mois (vide = pas d'alerte)."
  type        = string
  default     = ""
}

variable "max_concurrency" {
  description = "Exécutions simultanées réservées (0 = pas de réservation ; un compte neuf est déjà limité à 10 au total)."
  type        = number
  default     = 0
}

# The permanent free tier covers 25 read and 25 write capacity units for the whole account.
variable "read_capacity" {
  type    = number
  default = 15
}

variable "write_capacity" {
  type    = number
  default = 10
}
