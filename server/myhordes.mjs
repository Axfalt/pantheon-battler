// Who is this MyHordes player? MyHordes' external API answers from the player's own key
// ("userkey") and this application's key ("appkey", given by the MyHordes team):
//   GET https://myhordes.eu/api/x/json/me?appkey=…&userkey=… → { id, name, … }
// The player's key is used for this one call, never stored nor logged.
//
// A verifier is `async (userKey) => ({ id, name }) | ({ error, status })`.

const ERRORS = {
  invalid_userkey: [401, "Clé MyHordes refusée : vérifiez-la dans votre compte MyHordes."],
  invalid_appkey: [503, "Le serveur du Bazar n'est pas (ou plus) reconnu par MyHordes : prévenez un MJ."],
  nightly_attack: [503, "MyHordes est en pleine attaque nocturne : réessayez dans quelques minutes."],
  rate_limit_reached: [503, "MyHordes reçoit trop de demandes : réessayez dans une minute."],
};

export function myHordesVerifier({ appKey, base = "https://myhordes.eu/api/x/json", fetchImpl = globalThis.fetch } = {}) {
  if (!appKey) return null;
  return async function verify(userKey) {
    const url = `${base.replace(/\/+$/, "")}/me?appkey=${encodeURIComponent(appKey)}&userkey=${encodeURIComponent(userKey)}`;
    let r;
    let j = null;
    try {
      r = await fetchImpl(url, { headers: { accept: "application/json" }, signal: AbortSignal.timeout(8000) });
      j = await r.json();
    } catch (e) {
      return { status: 503, error: "MyHordes ne répond pas : réessayez plus tard, ou continuez sans clé." };
    }
    if (j && j.error) {
      const [status, error] = ERRORS[j.error] || [502, `MyHordes a refusé la demande (${String(j.error).slice(0, 40)}).`];
      return { status, error };
    }
    const id = j && (typeof j.id === "number" || typeof j.id === "string") ? String(j.id) : "";
    const name = j && typeof j.name === "string" ? j.name.trim() : "";
    if (!r.ok || !/^\d{1,12}$/.test(id) || !name) return { status: 502, error: "Réponse de MyHordes illisible : réessayez plus tard." };
    return { id, name };
  };
}

/// For the local test server: the key "test-Alice" is the MyHordes player Alice.
export function fakeMyHordes() {
  return async function verify(userKey) {
    const m = /^test-(.{1,40})$/.exec(userKey);
    if (!m) return { status: 401, error: "Clé MyHordes refusée (serveur local : écrivez test-VotreNom)." };
    let id = 7;
    for (const c of m[1].toLowerCase()) id = (id * 31 + c.codePointAt(0)) % 999983;
    return { id: String(id), name: m[1] };
  };
}
