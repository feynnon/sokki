// Sokki hook: a meeting's action items into Apple Reminders (HOOKS.md).
// on = "meeting.note_ready", scope = ["tasks"]. macOS only.
//
// - One reminder per open action item, in the list REMINDERS_LIST ("Sokki"
//   unless set; made when missing). An item that is already there and not
//   done is skipped, so a reprocessed meeting adds nothing twice.
// - REMINDERS_ONLY="Ich,Sam": only items for these owners, plus items
//   without an owner. Unset: every item, with its owner in front
//   ("Anna: Book the hall").
// - SOKKI_HOOK_TEST=1 (`sokki hooks test`): prints what it would add and
//   leaves Reminders alone.
//
// The first real run asks once whether Sokki may control Reminders. If
// Reminders was closed, it is closed again afterwards.

ObjC.import("Foundation");

function env(name) {
  const v = $.NSProcessInfo.processInfo.environment.objectForKey(name);
  return v.isNil() ? "" : v.js;
}

function readStdin() {
  const data = $.NSFileHandle.fileHandleWithStandardInput.readDataToEndOfFile;
  return $.NSString.alloc.initWithDataEncoding(data, $.NSUTF8StringEncoding).js;
}

function run() {
  const event = JSON.parse(readStdin());
  const tasks = (event.content && event.content.tasks) || [];
  const meta = event.meta || {};
  const only = env("REMINDERS_ONLY")
    .split(",")
    .map((s) => s.trim().toLowerCase())
    .filter(Boolean);
  const listName = env("REMINDERS_LIST") || "Sokki";
  const from = `${meta.title || "Meeting"}, ${(meta.started_at || event.at || "").slice(0, 10)} (Sokki)`;

  const names = tasks
    .filter((t) => !t.done)
    .filter((t) => !only.length || !t.owner || only.includes(t.owner.toLowerCase()))
    .map((t) => (t.owner && !only.length ? `${t.owner}: ${t.text}` : t.text));

  if (env("SOKKI_HOOK_TEST")) {
    names.forEach((n) => console.log(`would add to ${listName}: ${n} [${from}]`));
    if (!names.length) console.log("no open action items");
    return;
  }
  if (!names.length) {
    console.log("no open action items");
    return;
  }

  const Reminders = Application("Reminders");
  const wasRunning = Reminders.running();
  const found = Reminders.lists.whose({ name: listName })();
  let list = found.length ? found[0] : null;
  if (!list) {
    Reminders.lists.push(Reminders.List({ name: listName }));
    list = Reminders.lists.whose({ name: listName })()[0];
  }
  for (const name of names) {
    if (list.reminders.whose({ name: name, completed: false })().length) {
      console.log(`already there: ${name}`);
      continue;
    }
    list.reminders.push(Reminders.Reminder({ name: name, body: from }));
    console.log(`added: ${name}`);
  }
  if (!wasRunning) Reminders.quit();
}
