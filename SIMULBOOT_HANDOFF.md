# Simulboot — Claude Code Handoff Document

## Read this first

This document is the complete brief for implementing simulboot v0. It was produced
after an extended design session covering the full substrate architecture. The demo
is a specific, constrained scope — do not implement anything not listed here.

---

## What simulboot is (one paragraph)

Simulboot is a proof-by-construction counter-argument to Stephen Kell's conclusion
in "The Operating System: Should There Be One?" (PLOS 2013). Kell argues (following
Ingalls) that operating systems should converge toward better composition, but
concludes that an OS is still necessary. Simulboot's counter-claim: the OS is
necessary on each machine as infrastructure, but the thing the user inhabits — the
session — should be separable from any specific OS instance, portable across machines,
and self-describing. This is the Squeak/Pharo image model applied correctly: not
absorbing the OS into the language, but separating the session from the OS. OS
instances are ports. The session is the ship. It travels the seven seas.

The demo shows this by: running a unified compositor on a MacBook that simultaneously
displays surfaces from macOS, Windows, and Linux; suspending the session by closing
the MacBook; resuming the session on a second device; watching all three OS surfaces
reconstitute automatically.

---

## Intellectual lineage (for context, not implementation)

- **Kell (2013)**: "The Operating System: Should There Be One?" — the paper being
  counter-argued. Kell's method: find the hidden universal abstraction (the allocator
  in liballocs), make it explicit and uniform. We apply the same method one level up:
  the surface is the universal abstraction at the session level.
- **liballocs**: Kell's own tool — "meta-level run-time services for Unix processes."
  The meta-DSO (a separately-loadable type description) is the model for the session
  image (a separately-loadable session description).
- **Squeak/Pharo image model**: the session is a portable computational world that
  travels across machines. Not a Squeak replacement — the conceptual model.
- **Niri compositor**: scrollable infinite horizontal strip layout. Windows/surfaces
  append to the right; existing surfaces never resize when new ones arrive.
- **Yoshida's Scribble**: multiparty session types specification language. Used to
  formally specify the simulboot wire protocol.

---

## What the demo must prove

Three claims. Every requirement below serves at least one:

**Claim A**: The session is separable from the OS. It can span multiple OS instances
without being bound to any one, and migrate across compositor instances.

**Claim B**: All surfaces are uniform regardless of OS origin or capture mechanism.
The compositor has no macOS-specific, Windows-specific, or Linux-specific code paths.

**Claim C**: OS instances are infrastructure, not the thing the user inhabits. They
persist through session suspension and reconnect automatically on resumption.

---

## Functional requirements

### Session identity and portability

- **F1**: The substrate maintains a *session* — a named, persistent computational
  environment with stable identity across time and compositor instances.

- **F2**: The session draws on OS instances as surface sources without being bound to
  any of them. The session outlives any individual OS instance's participation.

- **F3**: The session can be *suspended* — checkpointed to a content-addressed
  persistent image that fully describes its state.

- **F4**: A suspended session can be *resumed* on a different compositor instance. On
  resumption, the compositor loads the session image and reconstitutes the session:
  surface connections re-established, strip layout restored, focus state restored.

- **F5**: Source OS instances remain running during session suspension. When the
  session resumes, hosts reconnect automatically to the new compositor instance.
  (Proves Claim C.)

- **F6**: The session image is *self-describing*: it contains sufficient information
  for a bare compositor to reconstitute the session without external configuration.
  No TOML file required on resume.

### Surface uniformity

- **F7**: Every surface is uniform from the compositor's perspective regardless of OS
  origin or capture mechanism. The compositor has no macOS-specific, Windows-specific,
  or Linux-specific code paths in rendering or input routing. (Proves Claim B.)

- **F8**: Every surface's *provenance is queryable*: which host produced it, which OS,
  which machine. Stored in the session image.

- **F9**: Surfaces are produced by hosts. A host implements one interface —
  SurfaceAnnounce, ContentFrame, InputEvent — regardless of underlying capture
  mechanism.

### Strip layout

- **F10**: Surfaces arranged in a horizontally scrollable infinite strip. New surfaces
  append to the right. Existing surfaces never resize when new surfaces arrive or
  disconnect.

- **F11**: Two-finger trackpad swipe scrolls the strip. Scroll position is part of
  session state and preserved in the session image.

- **F12**: Click focuses a surface. Focus state is part of session state and preserved
  in the session image.

- **F13**: A disconnecting surface disappears cleanly; remaining surfaces unaffected.
  Reconnecting surface appears at the right end.

### Input routing

- **F14**: Keyboard input forwarded to focused surface's host and injected into the
  source OS. Input injection is the host's responsibility; the compositor sends
  normalised InputEvent structs uniformly.

- **F15**: Mouse input forwarded to the surface under the cursor, normalised to 0.0–1.0
  within that surface's bounds. Host denormalises for injection.

### v0 instantiation

- **F16**: Compositor runs on Apple Silicon MacBook (target: M-series). Three sources:
  - macOS: same MacBook, CGVirtualDisplay + ScreenCaptureKit, capturing a specific
    application window (e.g. Safari), loopback via localhost QUIC
  - Windows: PC on Tailscale, WGC + hardware encode (NVENC/AMF)
  - Linux: same PC, Hyper-V VM booted from physical Linux drive via HCS API, captured
    via WGC

- **F17**: Demo must show suspension on the MacBook and resumption on a second Apple
  Silicon device.

---

## Non-functional requirements

- **N1**: Typing latency in remote surfaces: subjectively acceptable. Not measured.
- **N2**: Video quality: text legible. Not pixel-perfect.
- **N3**: No crashes during a 30-minute demo session.
- **N4**: Session checkpoint completes in under 5 seconds.
- **N5**: Session reconstitution on local tailnet: under 30 seconds.
- **N6**: Cold start to three surfaces visible: under 5 minutes.

---

## Explicitly out of scope for v0

Do not implement any of the following:

- Audio
- Clipboard sharing
- Drag and drop between surfaces
- More than three simultaneous surfaces
- Linux as a bare-metal host (Linux VM via HCS API on Windows is sufficient)
- OxCaml (Rust throughout for v0)
- Session types (use simple enum messages instead)
- Graded comonad / product lattice / coeffect system
- WASM firm tier
- Cap'n Proto wire format (use bincode for v0)
- Content hash deduplication on the content channel
- Morphism algebra (no translate/composite/resample morphisms)
- Reactive DAG (no push-pull FRP)
- DRR scheduler / hard floor / mode switch
- Elliott stability intervals
- CRDT weaves
- Full capability model (CHERI, IFC lattice)
- Adaptive sync / HDR / HiDPI of remote surfaces
- Broker UI (session image serves as broker for v0)
- Performance optimisation beyond "it works"

---

## Architecture: components

```
simulboot/
├── simulboot-common/     # Shared types: wire protocol, session image types
├── simulboot-host/       # Runs on each source machine
│   ├── src/macos/        # CGVirtualDisplay + SCK + VideoToolbox
│   ├── src/windows/      # WGC + NVENC/AMF; HCS API VM manager
│   └── src/linux/        # PipeWire + VA-API (future, not blocking demo)
├── simulboot-client/     # Compositor, runs on MacBook
│   ├── src/compositor/   # Metal renderer, strip layout, input routing
│   ├── src/session/      # Session image load/save, checkpoint, restore
│   └── src/network/      # QUIC connection management
└── simulboot-broker/     # Minimal HTTP server serving session image (v0)
```

All Rust. Use Cargo workspace.

---

## Wire protocol

### Transport

- **Tailscale** for the overlay network (mutual auth, NAT traversal, encryption)
- **Quinn** (QUIC) for the transport layer
- Two channels per surface:
  - **Structure channel**: QUIC reliable stream, carries control messages
  - **Content channel**: QUIC datagrams, carries encoded frames

### Shared types (simulboot-common/src/lib.rs)

```rust
use serde::{Deserialize, Serialize};

pub type SurfaceId = [u8; 32]; // SHA-256 content hash

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Codec {
    H265,
    AV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OsKind {
    MacOS,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostProvenance {
    pub os: OsKind,
    pub machine_name: String,
    pub tailscale_addr: String,
    pub capture_description: String, // e.g. "window:Safari", "display:0", "vm:PhysicalDrive1"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceAnnounce {
    pub id: SurfaceId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub codec: Codec,
    pub provenance: HostProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHeader {
    pub surface_id: SurfaceId,
    pub pts: u64,
    pub len: u32, // byte length of encoded frame immediately following this header
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputEvent {
    KeyDown { keycode: u32 },
    KeyUp { keycode: u32 },
    MouseMove { x: f32, y: f32 }, // normalised 0.0–1.0 within surface bounds
    MouseDown { button: u8 },
    MouseUp { button: u8 },
    Scroll { dx: f32, dy: f32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StructureMessage {
    // Host → Compositor
    Announce(SurfaceAnnounce),
    SuspendAck,
    ReconnectOk(SurfaceAnnounce),
    ReconnectFail { reason: String },

    // Compositor → Host
    InputEvent { surface_id: SurfaceId, event: InputEvent },
    Resize { surface_id: SurfaceId, width: u32, height: u32 },
    Suspend,
    Reconnect { session_id: String },
    Disconnect,
}
```

Wire format: **bincode** over length-prefixed frames on the QUIC stream.
Content channel: raw encoded bytes with `FrameHeader` prepended (also bincode).

Replace bincode with Cap'n Proto in v1 when zero-copy parsing matters.

---

## Session image format

### Abstract model: XML Information Set

The session image is specified as an XML document. The canonical form (W3C C14N)
is used for content addressing: `session_id = SHA256(C14N(document))`.

### Storage and transmission

- **Text XML**: human-readable, for debugging and version control
- **Fast Infoset**: binary encoding of the same Infoset, for storage in the
  persistence domain and network transmission
- **C14N**: canonical form, for content addressing

### Schema (versioned XSD namespace)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<session xmlns="https://simulboot.dev/session/v1"
         id="sha256:..."
         created="2026-06-28T19:44:09Z"
         schema-version="1">

  <surfaces>
    <surface id="sha256:..." name="macOS" order="0">
      <host>
        <address>127.0.0.1:7001</address>
        <os>macOS</os>
        <machine>Aleth-MacBook</machine>
        <capture>window:Safari</capture>
      </host>
      <codec>H265</codec>
      <dimensions width="960" height="540"/>
    </surface>

    <surface id="sha256:..." name="Windows" order="1">
      <host>
        <address>100.x.x.y:7001</address>
        <os>Windows</os>
        <machine>Aleth-PC</machine>
        <capture>display:0</capture>
      </host>
      <codec>H265</codec>
      <dimensions width="960" height="540"/>
    </surface>

    <surface id="sha256:..." name="Linux" order="2">
      <host>
        <address>100.x.x.y:7002</address>
        <os>Linux</os>
        <machine>Aleth-PC</machine>
        <capture>vm:PhysicalDrive1</capture>
      </host>
      <codec>H265</codec>
      <dimensions width="960" height="540"/>
    </surface>
  </surfaces>

  <layout>
    <strip scroll-pos="0.0"/>
    <focus surface-ref="sha256:..."/>
  </layout>

</session>
```

### Session image discovery (Q2 resolved)

The suspending compositor runs a minimal HTTP server on its Tailscale address:

```
GET http://{tailscale_hostname}:7000/session/{session_id_hex}
→ 200 OK, Content-Type: application/fastinfoset
```

The resuming compositor is launched with `--resume http://...` flag.
The server stays up until all hosts have acknowledged Reconnect.

---

## Scribble protocol specifications

These are the multiparty session type specifications for the wire protocol.
They serve as the formal contract for the implementation.

### SimulbootSession (ongoing operation)

```scribble
global protocol SimulbootSession(
  role Compositor,
  role macOSHost,
  role WindowsHost,
  role LinuxHost
) {
  SurfaceAnnounce(SurfaceInfo) from macOSHost to Compositor;
  SurfaceAnnounce(SurfaceInfo) from WindowsHost to Compositor;
  SurfaceAnnounce(SurfaceInfo) from LinuxHost to Compositor;

  rec Streaming {
    choice at Compositor {
      or { ContentFrame(FrameData) from macOSHost to Compositor;
           continue Streaming; }
      or { ContentFrame(FrameData) from WindowsHost to Compositor;
           continue Streaming; }
      or { ContentFrame(FrameData) from LinuxHost to Compositor;
           continue Streaming; }
      or { InputEvent(EventData) from Compositor to macOSHost;
           continue Streaming; }
      or { InputEvent(EventData) from Compositor to WindowsHost;
           continue Streaming; }
      or { InputEvent(EventData) from Compositor to LinuxHost;
           continue Streaming; }
      or {
        Suspend() from Compositor to macOSHost;
        Suspend() from Compositor to WindowsHost;
        Suspend() from Compositor to LinuxHost;
        SuspendAck() from macOSHost to Compositor;
        SuspendAck() from WindowsHost to Compositor;
        SuspendAck() from LinuxHost to Compositor;
      }
    }
  }
}
```

### SessionResumption

```scribble
global protocol SessionResumption(
  role NewCompositor,
  role Store,
  role macOSHost,
  role WindowsHost,
  role LinuxHost
) {
  SessionImageRequest(SessionId) from NewCompositor to Store;
  SessionImage(XmlInfoset) from Store to NewCompositor;

  Reconnect(SessionId) from NewCompositor to macOSHost;
  Reconnect(SessionId) from NewCompositor to WindowsHost;
  Reconnect(SessionId) from NewCompositor to LinuxHost;

  choice at macOSHost {
    or { ReconnectOk(SurfaceInfo) from macOSHost to NewCompositor; }
    or { ReconnectFail(Reason) from macOSHost to NewCompositor; }
  }
  choice at WindowsHost {
    or { ReconnectOk(SurfaceInfo) from WindowsHost to NewCompositor; }
    or { ReconnectFail(Reason) from WindowsHost to NewCompositor; }
  }
  choice at LinuxHost {
    or { ReconnectOk(SurfaceInfo) from LinuxHost to NewCompositor; }
    or { ReconnectFail(Reason) from LinuxHost to NewCompositor; }
  }
}
```

---

## Platform-specific implementation notes

### macOS host (critical details)

**CGVirtualDisplay**

- Private CoreGraphics API. Used by BetterDisplay, Lumen, Apple's own Sidecar.
- Creates a virtual display independent of the physical panel — lid open/close
  does not interrupt capture.
- Run CGVirtualDisplay creation in a **subprocess** (`vd-helper`) for stability.
  If the main host process crashes, the virtual display persists.
- Physical dimensions matter: CGVirtualDisplay rejects displays where PPI exceeds
  a threshold. Declare 27-inch equivalent (597×336mm) for any resolution up to 4K.
  See Lumen's vd_helper implementation for the exact API calls.
- CGVirtualDisplay is macOS 14+. Target macOS 14 minimum.

**SCK (ScreenCaptureKit)**

- TCC-gated only: no entitlement grants capture permission. User must enable
  in System Settings → Privacy & Security → Screen & System Audio Recording.
- Use `SCContentSharingPicker` to let the user select the capture target (the
  powerbox pattern). Store the selection in the session image.
- Frames arrive as `CMSampleBuffer` containing `CVPixelBuffer` backed by `IOSurface`.
  Pass the `IOSurface` directly to VideoToolbox — zero CPU copy.
- For unattended operation: request `com.apple.developer.persistent-content-capture`
  entitlement from Apple. Takes weeks. Request early.
- Full capture (60fps) costs ~1.9% of one CPU core on Apple Silicon.

**VideoToolbox encode**

- Hardware H.265 encode via `VTCompressionSession`.
- Feed `IOSurface` directly from SCK — zero copy path.
- Target: H.265 at the stream's native resolution, 60fps.

**IOPMAssertion**

- Hold `kIOPMAssertionTypePreventUserIdleSystemSleep` while the host is running.
  Prevents system sleep when lid closes. Required for the suspension flow to work
  (compositor must be able to send Suspend to all hosts before sleeping).

**Input injection**

- Use `CGEventPost(kCGHIDEventTap, event)` for keyboard and mouse events.
- For the focused surface: denormalise mouse coordinates from 0.0–1.0 to the
  virtual display's pixel space before posting.

**Quinn (QUIC) on macOS**

- TLS certificate: generate self-signed cert with `rcgen` at first run, persist
  to disk. Client skips verification (Tailscale already authenticates the peer).
- The host listens on `0.0.0.0:7001` (or configured port).
- The compositor connects to the host's Tailscale IP.

---

### Windows host (critical details)

**Session isolation**

- WGC (Windows.Graphics.Capture) requires the process to be in Session 1
  (the interactive user session). Run simulboot-host as a regular user-mode
  `.exe` launched from the desktop, NOT as a Windows Service.
- Verify at startup: `WTSGetActiveConsoleSessionId()` must return 1.
  If it returns 0, the process is in Session 0 and WGC will fail.

**WGC (Windows.Graphics.Capture)**

- Use `GraphicsCapturePicker` for user-consented window/display selection.
- Frames arrive as `IDirect3DSurface` GPU textures — zero CPU copy path.
- Resilient to physical display state changes (captures from DWM, not display
  hardware). Does not break on monitor on/off.
- The Yellow border around captured windows can be disabled on Windows 11 22H2+
  via `IGraphicsCaptureSession3::IsBorderRequired = false`.
- Use the `windows` crate for all Win32/WinRT bindings.

**Hardware encode**

- NVENC via the `windows` crate + Direct3D 11 interop (if NVIDIA GPU present).
- AMF via AMD's SDK (if AMD GPU).
- Intel Quick Sync via MFT (Media Foundation Transform).
- For v0: detect GPU vendor at startup, use the available encoder.
- Feed `IDirect3DSurface` directly to the encoder — zero copy.

**HCS API for Linux VM**

- Use the Host Compute System API (`computecore.dll`) to create a Hyper-V VM
  from the physical Linux SSD.
- No registration in Hyper-V Manager — HCS API is stateless.
- JSON configuration (NanaBox pattern):

```json
{
  "NanaBox": {
    "GuestType": "Linux",
    "MemorySize": 8192,
    "ProcessorCount": 4,
    "ScsiDevices": [
      {
        "Type": "PhysicalDevice",
        "Path": "\\\\.\\PhysicalDrive1"
      }
    ],
    "Gpu": {
      "AssignmentMode": "Default"
    },
    "NetworkAdapters": [
      {
        "Connected": true,
        "EndpointId": "generate-new-uuid-here",
        "MacAddress": "00-15-5D-47-EB-01"
      }
    ],
    "Type": "VirtualMachine",
    "Version": 1
  }
}
```

- The Linux SSD must be a Hyper-V Gen 2 compatible installation: UEFI, GPT,
  hv_vmbus/hv_storvsc/hv_netvsc drivers in the kernel. Most modern Linux
  distributions (Ubuntu 20.04+, Fedora 32+, Arch) meet this requirement.
- GPU-PV (paravirtualization): the Linux VM shares the host GPU. The VM's
  rendered output goes through the host GPU driver.
- Capture the VM's display window via WGC — same path as native Windows surfaces.
- The Linux VM host runs on port 7002 (separate from the Windows native host on
  7001).

**Input injection**

- Windows native surface: `SendInput` Win32 API.
- Linux VM surface: forward input events to the VM via the HCS API input channel,
  or inject via RDP into the VM session.

---

### Compositor / client (macOS, critical details)

**Metal rendering**

- Use `wgpu` with Metal backend, or direct Metal via `metal` crate.
- At vsync: iterate surfaces in strip order, composite each decoded frame
  (IOSurface-backed `MTLTexture`) into the render pass.
- The strip is a `Vec<Surface>` sorted by `order`. Render only the visible
  portion (the viewport window into the infinite strip).

**Strip layout**

```rust
struct Strip {
    surfaces: Vec<Surface>,  // ordered by .order field
    scroll_pos: f32,         // pixels scrolled from left edge
    viewport_width: f32,
    viewport_height: f32,
}

struct Surface {
    id: SurfaceId,
    name: String,
    order: u32,
    width: f32,              // surface width in compositor pixels
    height: f32,
    frame: Option<GpuFrame>, // latest decoded frame
    host_addr: SocketAddr,
}
```

Default surface width: one-third of viewport width. Height: full viewport height.
Surfaces are adjacent with no gap. The viewport shows whichever surfaces fall
within `[scroll_pos, scroll_pos + viewport_width]`.

**Trackpad input**

- Two-finger horizontal swipe → update `strip.scroll_pos`. Do NOT forward to
  any surface host.
- Click on a surface → set focus to that surface's `SurfaceId`.
- All other input → forward to focused surface's host via the structure channel.

**VideoToolbox decode**

- Use `ffmpeg-next` crate with VideoToolbox backend for hardware H.265 decode.
  Simpler than raw VideoToolbox FFI for v0. Replace with direct VideoToolbox
  in v1 if latency is a concern.
- Decoded frames: `CVPixelBuffer` → `IOSurface` → `MTLTexture` (zero copy).

**Session checkpoint (suspension)**

1. Send `Suspend` to all hosts on structure channels.
2. Wait for `SuspendAck` from all hosts (or timeout after 5s).
3. Build the session XML document from current strip state.
4. Compute SHA256(C14N(document)) as the session ID.
5. Write the session image to disk (text XML for debugging, Fast Infoset for
   serving).
6. Start the HTTP server on port 7000 serving the session image.
7. Display "Session suspended. Resume at: http://{tailscale_ip}:7000/session/{id}"
8. Exit (or enter suspend state — for v0, exit is fine).

**Session resumption**

1. Fetch session image from `--resume` URL.
2. Parse XML (use `quick-xml` crate), validate structure.
3. Verify SHA256(C14N(document)) matches the session ID in the URL.
4. For each surface in the session image, open a QUIC connection to the host's
   address.
5. Send `Reconnect { session_id }` on the structure channel.
6. Wait for `ReconnectOk` — host responds with a fresh `SurfaceAnnounce`.
7. Restore strip order, scroll position, focus from the session image.
8. Begin receiving frames — session is live.

---

## Build order (8 weeks)

**Week 1: QUIC loopback on MacBook**
- `cargo new --workspace simulboot`
- Add `simulboot-common`, `simulboot-host`, `simulboot-client` crates
- Implement `StructureMessage` and `FrameHeader` types in simulboot-common
- Quinn QUIC connection: simulboot-host and simulboot-client both on MacBook
- Structure channel: host sends `Announce`, client receives it
- No video yet — just connection establishment

**Week 2: macOS capture pipeline**
- CGVirtualDisplay creation in `vd-helper` subprocess
- SCK stream capturing a specific application window
- VideoToolbox H.265 encode
- Encoded frames sent over QUIC content channel (datagrams)
- One surface visible on MacBook's own screen (Metal window)

**Week 3: Metal compositor**
- `wgpu` Metal render pipeline
- VideoToolbox decode: encoded bytes → IOSurface → MTLTexture
- Strip layout: single surface fills viewport
- One surface displayed. macOS loopback working end-to-end.

**Week 4: Windows source**
- simulboot-host on PC (Windows)
- WGC capture + NVENC encode
- QUIC over Tailscale to MacBook
- Two surfaces in compositor: macOS (loopback) + Windows
- Input routing: click Windows surface → SendInput on PC

**Week 5: Linux VM source**
- HCS API VM creation from physical Linux drive on Windows PC
- WGC capture of VM window
- Same QUIC path as Windows native host (different port)
- Three surfaces in compositor: macOS + Windows + Linux VM
- Input routing to all three

**Week 6: Strip layout and input**
- Full niri-style scrollable strip
- Two-finger trackpad swipe scrolls strip
- Click to focus
- Keyboard routing to focused surface's host
- Surface labels (OS name) overlaid

**Week 7: Session suspend and resume**
- Suspension flow (send Suspend → SuspendAck → checkpoint → HTTP server)
- XML session image generation (use `quick-xml`)
- C14N hash computation (implement minimal C14N or use `xml-c14n` crate)
- Resumption flow (fetch image → parse → Reconnect → ReconnectOk → restore)
- Test: suspend on MacBook, resume on MacBook (same machine first)
- Test: suspend on MacBook, resume on second Apple Silicon device

**Week 8: Demo polish**
- IOPMAssertion (prevent MacBook sleep during active session)
- Reconnection on transient network hiccup (retry with backoff)
- Graceful host disconnect (surface fades out, strip closes gap)
- "Session suspended at URL X" display
- Fix everything that breaks during a 30-minute run

---

## Key dependencies

```toml
# simulboot-common
serde = { version = "1", features = ["derive"] }
bincode = "1"
quick-xml = "0.31"

# simulboot-host (macOS target)
screencapturekit = "0.3"      # SCK bindings
objc2 = "0.5"                 # Objective-C runtime (for CGVirtualDisplay FFI)
core-foundation = "0.9"
core-graphics = "0.23"
ffmpeg-next = "7"             # VideoToolbox encode via ffmpeg

# simulboot-host (Windows target)
windows = { version = "0.58", features = [
  "Win32_Graphics_Direct3D11",
  "Win32_Graphics_Dxgi",
  "Win32_Graphics_Capture",
  "Win32_System_WinRT",
  "Win32_Foundation",
  "Win32_System_Power",
] }

# simulboot-client
wgpu = "0.20"
winit = "0.29"
quinn = "0.11"
ffmpeg-next = "7"

# both
tokio = { version = "1", features = ["full"] }
quinn = "0.11"
rustls = "0.23"
rcgen = "0.13"
serde_json = "1"
```

---

## What "done" looks like

The demo is complete when the following sequence works without manual intervention:

1. Start simulboot-host on MacBook (captures Safari window)
2. Start simulboot-host on PC Windows (captures Windows Explorer or any app)
3. Start simulboot-host on PC Windows on port 7002 (HCS API boots Linux VM,
   captures Linux desktop)
4. Start simulboot-client on MacBook with config pointing to all three hosts
5. Three surfaces appear in the scrollable strip on the MacBook
6. Scroll between surfaces, type in each one, see input arrive correctly
7. Press the "Suspend" key binding (e.g. Cmd+Shift+S)
8. MacBook prints: "Session suspended at http://100.x.x.x:7000/session/sha256:..."
9. On a second Apple Silicon device, run:
   `simulboot-client --resume http://100.x.x.x:7000/session/sha256:...`
10. Three surfaces reconstitute on the second device
11. All three OS instances are still running; hosts reconnect automatically
12. User resumes work on the second device

This demonstrates Claim A (session separated from OS), Claim B (surfaces uniform),
and Claim C (OS instances are infrastructure). The demo is complete.

---

## What NOT to ask Claude Code to do

- Do not implement the substrate (sections 1-6 of the design document)
- Do not implement OxCaml components
- Do not implement Cap'n Proto serialisation
- Do not implement the morphism algebra
- Do not implement session types beyond what's described in the wire protocol
- Do not implement the graded comonad or product lattice
- Do not implement WASM support
- Do not implement the content-addressed deduplication optimisation
- Do not implement the reactive DAG
- Do not implement DRR scheduling

These are all correct long-term designs. They do not appear in v0.

---

*Document produced from design session on 2026-06-28. Full session transcript
available if deeper context is needed on any decision.*
