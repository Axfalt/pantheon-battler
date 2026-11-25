// AWS Lambda entry point, called through its Function URL (same event shape as API Gateway's HTTP
// API, payload 2.0). The Function URL answers the browser's CORS checks itself.
//
// Environment (terraform/main.tf): TABLE_NAME, GM_KEY, MH_APPKEY (empty: no MyHordes login),
// TTL_DAYS (how long an unchanged item is kept).

import { createHandler } from "./handler.mjs";
import { dynamoStore } from "./store-dynamo.mjs";
import { myHordesVerifier } from "./myhordes.mjs";

const ttlDays = Number(process.env.TTL_DAYS) || 60;
const handle = createHandler(dynamoStore({ table: process.env.TABLE_NAME, ttlDays }), {
  ttlDays,
  gmKey: process.env.GM_KEY,
  myHordes: myHordesVerifier({ appKey: process.env.MH_APPKEY || "" }),
});

export async function handler(event) {
  const body = event.body && event.isBase64Encoded ? Buffer.from(event.body, "base64").toString("utf8") : event.body || "";
  const r = await handle({
    method: event.requestContext.http.method,
    path: event.rawPath,
    query: event.queryStringParameters || {},
    headers: event.headers || {},
    body,
  });
  return {
    statusCode: r.status,
    headers: { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" },
    body: JSON.stringify(r.body),
  };
}
