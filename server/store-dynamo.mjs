// The Bazar store in DynamoDB: one table, one partition ("bazar"), these items (sort key sk):
//   seq                        { n }: the last event number
//   shop                       { shop }: the current shop
//   stock                      { stock }: the GM's reserve and the last event it includes
//   avail                      { counts }: copies available, unit id → n
//   slot#<shop>#<level>#<slot> { player, seq, ... }: who bought a slot of a shop
//   ev#<seq, 12 digits>        { e }: an event
//   acct#<account id>          { a, v }: a player's account (god, name…) and its version
//   posts#<account id>         { posts }: its last posts (written with a post or a cancellation)
//   sess#<sha-256 of a token>  { account, epoch }: a device logged in to an account
//   name#<folded name>         { account, verified }: the account a name belongs to
//   mh#<MyHordes user id>      { account }: the account of a MyHordes player
//   gmseq                      { n }: the last number of the GMs' journal
//   gm#<n, 12 digits>          { n, e }: an event of the GMs' journal
//   gmid#<event id>            { n }: that event id is in the journal (a resend is skipped)
// Every write is a transaction that checks what it was computed from (the event number, the
// account's version), so two players can't buy the same slot, nor the last copy of a unit, and
// two devices can't overwrite a god.
//
// Every item carries `ttl` (epoch seconds): DynamoDB deletes it `ttlDays` after its last write
// (the table's TTL setting, template.yaml). The deletion can come a few days late, so reads and
// conditions treat an expired item as absent.

import { DynamoDBClient } from "@aws-sdk/client-dynamodb";
import { BatchGetCommand, DeleteCommand, DynamoDBDocumentClient, GetCommand, PutCommand, QueryCommand, TransactWriteCommand } from "@aws-sdk/lib-dynamodb";
import { Conflict } from "./handler.mjs";

const PK = "bazar";
const pad = (n) => String(n).padStart(12, "0");
const evKey = (seq) => `ev#${pad(seq)}`;
const slotKey = (shop, level, slot) => `slot#${shop}#${level}#${slot}`;

export function dynamoStore({ table, client = null, ttlDays = 60, clock = () => Date.now() }) {
  const db = DynamoDBDocumentClient.from(client || new DynamoDBClient({}), { marshallOptions: { removeUndefinedValues: true } });
  const key = (sk) => ({ pk: PK, sk });
  const nowS = () => Math.floor(clock() / 1000);
  const ttl = () => nowS() + ttlDays * 86400;
  const alive = (item) => (item && (!item.ttl || item.ttl > nowS()) ? item : null);
  const TTL = { "#ttl": "ttl" };

  // The condition that the last event is still `expectSeq`.
  const seqCheck = (expectSeq, update) => ({
    ...(update
      ? {
          Update: {
            TableName: table,
            Key: key("seq"),
            UpdateExpression: "SET n = :next, #ttl = :ttl",
            ExpressionAttributeNames: TTL,
            ExpressionAttributeValues: { ":exp": expectSeq, ":next": update, ":ttl": ttl() },
          },
        }
      : { ConditionCheck: { TableName: table, Key: key("seq"), ExpressionAttributeValues: { ":exp": expectSeq } } }),
  });
  const withSeqCondition = (op, expectSeq) => {
    const body = op.Update || op.ConditionCheck;
    body.ConditionExpression = expectSeq === 0 ? "attribute_not_exists(n) OR n = :exp" : "n = :exp";
    return op;
  };

  // An account and what goes with it (handler.mjs, `writes`): its name, the session in use,
  // its MyHordes link. Expired items count as absent.
  function accountOps(w) {
    const ops = [];
    const kinds = [];
    if (!w) return { ops, kinds };
    const t = ttl();
    const id = w.item.id;
    const me = { ":me": id, ":now": nowS() };
    const names = { "#acc": "account", "#ttl": "ttl" };
    ops.push({
      Put: {
        TableName: table,
        Item: { ...key(`acct#${id}`), a: w.item, v: w.item.version, ttl: t },
        ...(w.expectVersion == null
          ? { ConditionExpression: "attribute_not_exists(sk) OR #ttl < :now", ExpressionAttributeNames: TTL, ExpressionAttributeValues: { ":now": nowS() } }
          : { ConditionExpression: "#v = :ev AND #ttl > :now", ExpressionAttributeNames: { "#v": "v", ...TTL }, ExpressionAttributeValues: { ":ev": w.expectVersion, ":now": nowS() } }),
      },
    });
    kinds.push("account");
    if (w.claim) {
      ops.push({
        Put: {
          TableName: table,
          Item: { ...key(`name#${w.claim.name}`), account: id, verified: w.claim.verified, ttl: t },
          ConditionExpression: "attribute_not_exists(sk) OR #ttl < :now OR #acc = :me" + (w.claim.takeover ? " OR #ver = :f" : ""),
          ExpressionAttributeNames: { ...names, ...(w.claim.takeover ? { "#ver": "verified" } : {}) },
          ExpressionAttributeValues: { ...me, ...(w.claim.takeover ? { ":f": false } : {}) },
        },
      });
      kinds.push("name");
    }
    if (w.release) {
      ops.push({
        Delete: { TableName: table, Key: key(`name#${w.release}`), ConditionExpression: "#acc = :me", ExpressionAttributeNames: { "#acc": "account" }, ExpressionAttributeValues: { ":me": id } },
      });
      kinds.push("name");
    }
    if (w.session) {
      ops.push({ Put: { TableName: table, Item: { ...key(`sess#${w.session}`), account: id, epoch: w.item.epoch || 0, ttl: t } } });
      kinds.push("account");
    }
    if (w.posts !== undefined) {
      ops.push({ Put: { TableName: table, Item: { ...key(`posts#${id}`), posts: w.posts, ttl: t } } });
      kinds.push("account");
    }
    if (w.mh) {
      ops.push({
        Put: {
          TableName: table,
          Item: { ...key(`mh#${w.mh}`), account: id, ttl: t },
          ConditionExpression: "attribute_not_exists(sk) OR #ttl < :now OR #acc = :me",
          ExpressionAttributeNames: names,
          ExpressionAttributeValues: me,
        },
      });
      kinds.push("mh");
    }
    return { ops, kinds };
  }

  async function get(sk) {
    const r = await db.send(new GetCommand({ TableName: table, Key: key(sk), ConsistentRead: true }));
    return alive(r.Item);
  }

  async function query(params) {
    const out = [];
    let start;
    do {
      const r = await db.send(new QueryCommand({ TableName: table, ExclusiveStartKey: start, ...params }));
      out.push(...r.Items);
      start = r.LastEvaluatedKey;
      if (params.Limit && out.length >= params.Limit) break;
    } while (start);
    return out;
  }

  async function run(items, kinds) {
    try {
      await db.send(new TransactWriteCommand({ TransactItems: items }));
    } catch (e) {
      if (e.name !== "TransactionCanceledException") throw e;
      const reasons = e.CancellationReasons || [];
      const failed = reasons.findIndex((r) => r && r.Code === "ConditionalCheckFailed");
      if (failed >= 0) throw new Conflict(kinds[failed] || "seq");
      if (reasons.some((r) => r && r.Code === "TransactionConflict")) throw new Conflict("seq");
      throw e;
    }
  }

  return {
    async snapshot() {
      const r = await db.send(
        new BatchGetCommand({ RequestItems: { [table]: { Keys: ["seq", "shop", "avail", "gmseq"].map(key), ConsistentRead: true } } }),
      );
      const by = Object.fromEntries((r.Responses[table] || []).filter((i) => alive(i)).map((i) => [i.sk, i]));
      return { seq: by.seq ? by.seq.n : 0, shop: by.shop ? by.shop.shop : null, avail: by.avail ? by.avail.counts : null, gm: by.gmseq ? by.gmseq.n : 0 };
    },
    async slots(shopId) {
      const items = await query({
        KeyConditionExpression: "pk = :pk AND begins_with(sk, :p)",
        ExpressionAttributeValues: { ":pk": PK, ":p": `slot#${shopId}#` },
        ConsistentRead: true,
      });
      return items.map(({ pk, sk, ttl, ...v }) => v);
    },
    async events({ after = null, limit = 500, last = null } = {}) {
      if (last != null) {
        const r = await db.send(
          new QueryCommand({
            TableName: table,
            KeyConditionExpression: "pk = :pk AND begins_with(sk, :p)",
            ExpressionAttributeValues: { ":pk": PK, ":p": "ev#" },
            ScanIndexForward: false,
            Limit: last,
            ConsistentRead: true,
          }),
        );
        return r.Items.map((i) => i.e);
      }
      const items = await query({
        KeyConditionExpression: "pk = :pk AND sk BETWEEN :a AND :b",
        ExpressionAttributeValues: { ":pk": PK, ":a": evKey(after + 1), ":b": evKey(999999999999) },
        Limit: limit,
        ConsistentRead: true,
      });
      return items.slice(0, limit).map((i) => i.e);
    },
    async event(seq) {
      const item = await get(evKey(seq));
      return item ? item.e : null;
    },
    async commit({ expectSeq, event, slot = null, shopId = null, freeSlot = null, cancels = null, delta = null, account = null }) {
      const t = ttl();
      const items = [withSeqCondition(seqCheck(expectSeq, event.seq), expectSeq)];
      const kinds = ["seq"];
      items.push({ Put: { TableName: table, Item: { ...key(evKey(event.seq)), e: event, ttl: t }, ConditionExpression: "attribute_not_exists(sk)" } });
      kinds.push("seq");
      if (slot) {
        items.push({ Put: { TableName: table, Item: { ...key(slotKey(slot.shop, slot.level, slot.slot)), ...slot, ttl: t }, ConditionExpression: "attribute_not_exists(sk)" } });
        kinds.push("slot");
      }
      if (shopId) {
        items.push({ ConditionCheck: { TableName: table, Key: key("shop"), ConditionExpression: "#s.#id = :sid", ExpressionAttributeNames: { "#s": "shop", "#id": "id" }, ExpressionAttributeValues: { ":sid": shopId } } });
        kinds.push("shop");
      }
      if (freeSlot) {
        items.push({
          Delete: {
            TableName: table,
            Key: key(slotKey(freeSlot.shop, freeSlot.level, freeSlot.slot)),
            ConditionExpression: "attribute_not_exists(sk) OR #q = :q",
            ExpressionAttributeNames: { "#q": "seq" },
            ExpressionAttributeValues: { ":q": freeSlot.seq },
          },
        });
        kinds.push("seq");
      }
      if (cancels != null) {
        items.push({
          Update: {
            TableName: table,
            Key: key(evKey(cancels)),
            UpdateExpression: "SET e.cancelled = :c, #ttl = :ttl",
            ConditionExpression: "attribute_exists(sk) AND attribute_not_exists(e.cancelled)",
            ExpressionAttributeNames: TTL,
            ExpressionAttributeValues: { ":c": { seq: event.seq, at: event.at, by: event.by }, ":ttl": t },
          },
        });
        kinds.push("cancelled");
      }
      const changes = Object.entries(delta || {});
      if (changes.length) {
        const names = { ...TTL };
        const values = { ":ttl": t };
        const sets = ["#ttl = :ttl"];
        const conds = [];
        changes.forEach(([id, n], k) => {
          names[`#u${k}`] = id;
          values[`:d${k}`] = n;
          sets.push(`counts.#u${k} = counts.#u${k} + :d${k}`);
          if (n < 0) {
            values[`:m${k}`] = -n;
            conds.push(`counts.#u${k} >= :m${k}`);
          }
        });
        items.push({
          Update: {
            TableName: table,
            Key: key("avail"),
            UpdateExpression: "SET " + sets.join(", "),
            ConditionExpression: conds.length ? conds.join(" AND ") : "attribute_exists(counts)",
            ExpressionAttributeNames: names,
            ExpressionAttributeValues: values,
          },
        });
        kinds.push("stock");
      }
      const acct = accountOps(account);
      items.push(...acct.ops);
      kinds.push(...acct.kinds);
      try {
        await run(items, kinds);
      } catch (e) {
        if (e instanceof Conflict && e.kind === "slot") {
          const r = await db.send(new GetCommand({ TableName: table, Key: key(slotKey(slot.shop, slot.level, slot.slot)), ConsistentRead: true }));
          throw new Conflict("slot", { player: r.Item && r.Item.player });
        }
        throw e;
      }
    },
    async putShop(shop) {
      if (shop) await db.send(new PutCommand({ TableName: table, Item: { ...key("shop"), shop, ttl: ttl() } }));
      else await db.send(new DeleteCommand({ TableName: table, Key: key("shop") }));
    },
    async putStock({ expectSeq, stock, avail }) {
      await run(
        [
          withSeqCondition(seqCheck(expectSeq, null), expectSeq),
          { Put: { TableName: table, Item: { ...key("stock"), stock, ttl: ttl() } } },
          { Put: { TableName: table, Item: { ...key("avail"), counts: avail, ttl: ttl() } } },
        ],
        ["seq", "seq", "seq"],
      );
    },

    // The GMs' journal.
    async gmJournal({ after = 0, limit = 300 } = {}) {
      const items = await query({
        KeyConditionExpression: "pk = :pk AND sk BETWEEN :a AND :b",
        ExpressionAttributeValues: { ":pk": PK, ":a": `gm#${pad(after + 1)}`, ":b": `gm#${pad(999999999999)}` },
        Limit: limit,
        ConsistentRead: true,
      });
      const last = await get("gmseq");
      return { events: items.filter((i) => alive(i)).slice(0, limit).map((i) => ({ n: i.n, e: i.e })), last: last ? last.n : 0 };
    },
    async gmAppend(events) {
      // The ids already in the journal are skipped (a resend after a lost answer).
      const r = await db.send(new BatchGetCommand({ RequestItems: { [table]: { Keys: events.map((e) => key(`gmid#${e.id}`)), ConsistentRead: true } } }));
      const have = new Set((r.Responses[table] || []).filter((i) => alive(i)).map((i) => i.sk.slice(5)));
      const fresh = events.filter((e) => !have.has(e.id));
      const cur = await get("gmseq");
      const start = cur ? cur.n : 0;
      if (!fresh.length) return { added: 0, last: start };
      const t = ttl();
      const items = [
        {
          Put: {
            TableName: table,
            Item: { ...key("gmseq"), n: start + fresh.length, ttl: t },
            // The counter is still the one read (an expired one counts as absent).
            ConditionExpression: start ? "n = :exp AND #ttl > :now" : "attribute_not_exists(n) OR #ttl < :now",
            ExpressionAttributeNames: TTL,
            ExpressionAttributeValues: start ? { ":exp": start, ":now": nowS() } : { ":now": nowS() },
          },
        },
      ];
      fresh.forEach((e, k) => {
        const n = start + k + 1;
        items.push({ Put: { TableName: table, Item: { ...key(`gm#${pad(n)}`), n, e, ttl: t } } });
        items.push({
          Put: {
            TableName: table,
            Item: { ...key(`gmid#${e.id}`), n, ttl: t },
            ConditionExpression: "attribute_not_exists(sk) OR #ttl < :now",
            ExpressionAttributeNames: TTL,
            ExpressionAttributeValues: { ":now": nowS() },
          },
        });
      });
      // A failed condition (another GM wrote meanwhile, or sent the same event): the handler
      // tries again from the start.
      await run(items, items.map(() => "seq"));
      return { added: fresh.length, last: start + fresh.length };
    },

    // Accounts.
    async account(id) {
      const item = await get(`acct#${id}`);
      return item ? item.a : null;
    },
    async session(hash) {
      const item = await get(`sess#${hash}`);
      return item ? { account: item.account, epoch: item.epoch || 0 } : null;
    },
    async posts(id) {
      const item = await get(`posts#${id}`);
      return item ? item.posts : [];
    },
    async nameOwner(name) {
      const item = await get(`name#${name}`);
      return item ? { account: item.account, verified: !!item.verified } : null;
    },
    async mhAccount(mhId) {
      const item = await get(`mh#${mhId}`);
      return item ? item.account : null;
    },
    async saveAccount(w) {
      const { ops, kinds } = accountOps(w);
      if (ops.length > 1) return run(ops, kinds);
      // Only the account (a plain save of the god): one conditional write, half the cost of a
      // transaction.
      try {
        await db.send(new PutCommand({ ...ops[0].Put }));
      } catch (e) {
        if (e.name === "ConditionalCheckFailedException") throw new Conflict("account");
        throw e;
      }
    },
    async putSession(hash, accountId, epoch = 0) {
      await db.send(new PutCommand({ TableName: table, Item: { ...key(`sess#${hash}`), account: accountId, epoch, ttl: ttl() } }));
    },
    async dropSession(hash) {
      await db.send(new DeleteCommand({ TableName: table, Key: key(`sess#${hash}`) }));
    },
  };
}
