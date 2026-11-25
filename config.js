// Pantheon Battler: where the Bazar server is. On this computer (node server/dev.mjs), the local
// test server; elsewhere, the deployed server (server/terraform, output api_url). The deployed
// server only answers the public site's address (allowed_origin).
"use strict";

window.PANTHEON_API = /^(localhost|127\.0\.0\.1|\[::1\])$/.test(location.hostname)
  ? `${location.origin}/api`
  : "https://piku7l44itkusv3wznqolxe4ay0zqrbl.lambda-url.eu-west-3.on.aws";
