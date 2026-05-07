function reqHandler(req: Request): Response {
  if (req.url.endsWith("/api/ws")) {
    let i = 0;

    const { socket: ws, response: resp } = Deno.upgradeWebSocket(req, {
      protocol: "json",
    });

    console.log("Opened");

    ws.onmessage = (m) => {
      console.log("Echo:", m.data);

      ws.send(m.data);

      i += 1;

      if (i % 5 === 0) {
        // Testing reconnection...
        console.warn("Closing!");
        ws.close();
      }
    };

    return resp;
  } else {
    return new Response(Deno.readFileSync("./keyboard.html"), {
      headers: [["Content-Type", "text/html"]],
    });
  }
}

Deno.serve(reqHandler, { port: 8000 });
