// Placeholder page until the React interface (#12): it shows that the bridge works.
const { invoke } = window.__TAURI_INTERNALS__;
const state = document.getElementById("state");
const bots = document.getElementById("bots");
let next = 0;

const send = (type, fields = {}) => invoke("send", { line: JSON.stringify({ id: ++next, type, ...fields }) });

async function listen() {
  for (;;) {
    const frame = JSON.parse(await invoke("recv"));
    if (frame.type === "hello") {
      // Also after the service started again: a new connection begins with hello.
      state.textContent = "Connected to the service.";
      await send("hello", { protocol: 1 });
      await send("list_bots");
    } else if (frame.type === "response" && frame.command === "list_bots" && frame.success) {
      bots.replaceChildren(...frame.data.bots.map((bot) => {
        const item = document.createElement("li");
        item.textContent = bot.name;
        return item;
      }));
    } else if (frame.type === "bots_changed") {
      await send("list_bots");
    }
  }
}

listen().catch((e) => { state.textContent = `The connection failed: ${e}`; });
