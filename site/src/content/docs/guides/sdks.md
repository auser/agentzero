---
title: Client SDKs
description: Thin HTTP/WebSocket client SDKs for Python, TypeScript, Swift, and Kotlin.
---

AgentZero ships thin client SDKs that talk to the [gateway](/guides/deployment/) over HTTP and WebSocket. No native compilation or FFI bindings needed — any platform that can make HTTP calls can control AgentZero.

## Available SDKs

| Language   | Transport        | Install                          |
|------------|-----------------|----------------------------------|
| Python     | HTTP + WebSocket | `pip install agentzero`          |
| TypeScript | HTTP + WebSocket | `npm install @agentzero/client`  |
| Swift      | HTTP + WebSocket | Swift Package Manager            |
| Kotlin     | HTTP + WebSocket | Maven Central                    |

## Quick start

All SDKs follow the same pattern: point at a running gateway and send messages.

### Python

```python
from agentzero import AgentZeroClient

client = AgentZeroClient("http://localhost:3000")
response = client.chat("What can you do?")
print(response.content)
```

### TypeScript

```typescript
import { AgentZeroClient } from "@agentzero/client";

const client = new AgentZeroClient("http://localhost:3000");
const response = await client.chat("What can you do?");
console.log(response.content);
```

## OpenAI-compatible API

The gateway exposes a drop-in `/v1/chat/completions` endpoint. Any existing OpenAI client library works out of the box — just point it at your gateway URL:

```python
from openai import OpenAI

client = OpenAI(base_url="http://localhost:3000/v1", api_key="unused")
response = client.chat.completions.create(
    model="default",
    messages=[{"role": "user", "content": "Hello!"}],
)
```

## Streaming

All SDKs support Server-Sent Events (SSE) for real-time streaming:

```python
for chunk in client.chat_stream("Tell me a story"):
    print(chunk.content, end="", flush=True)
```

## WebSocket pairing

For persistent connections, SDKs can pair with the gateway over WebSocket:

```python
async with client.pair() as session:
    response = await session.send("Hello!")
    print(response.content)
```

## API reference

The gateway ships interactive [Scalar API docs](https://github.com/scalar/scalar) at `/docs`. Start the gateway and visit `http://localhost:3000/docs` to explore all endpoints.
