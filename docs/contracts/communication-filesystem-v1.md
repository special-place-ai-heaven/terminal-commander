# TC communication protocol: shared-filesystem fallback

**Proposed, not implemented.** As of 2026-10-09. This extends the
[minimum coordinator proposal](communication-buckets-mvp.md); it does not add a
second authority, a blockchain or automatic Git synchronization.

## One protocol, several transports

Each harness, session or VM uses its own TC. One configured coordinator owns
consent, invitations, memberships, sequence numbers and publication receipts.
Typed operations and their semantics stay the same over direct embed, approved
IPC/host routes and filesystem mailboxes. Transport success is not publication
success; a committed coordinator receipt is the publication authority.

When networking is unavailable, peers may exchange requests and responses through
a directory that they can actually reach. Both directions require an approved
writable outbox and a readable route to the other side. A common Git remote or
separate worktrees do not supply that route.

For a shared checkout, configure an ignored `.tc-coordination/` directory
explicitly. Hosts map their own absolute path to one public rendezvous ID; Windows
`C:\project\.tc-coordination` and WSL
`/mnt/c/project/.tc-coordination` can be mappings to the same storage.
[Microsoft documents these cross-filesystem paths](https://learn.microsoft.com/en-us/windows/wsl/filesystems).
A VM needs a real host-provided shared mount. TC never mounts host disks or grants
itself permissions.

Git history is not the mailbox. No automatic commits, pushes, pulls or tracked
file changes are part of this protocol. Never place a shared live SQLite database
in the rendezvous: each TC keeps its own durable state locally.

## Exact-size records

The proposed filesystem profile has one record size: **4,096 bytes**.
Every published record is compact canonical UTF-8 JSON followed by ASCII spaces
to that exact size. Its filename is 64 lowercase SHA-256 hexadecimal characters
plus `.tccard`, hashing all 4,096 bytes including padding.

Use a closed schema with protocol version, kind, record ID, operation/request ID,
sender principal and session generation, intended recipient/coordinator, channel
ID when applicable, consent revision, expiry, codec, decoded length, bounded
payload, key ID and signature. Body/purpose text is data. Contact information
names a preconfigured rendezvous or mailbox ID; it cannot supply an arbitrary
path, URL, executable, shell command or credential.

Every complete serialized JSON object must fit before padding. Reject an
oversized object; never truncate it into a valid-looking record. Proposed upper
bounds are 3,072 bytes for the serialized payload field and 16 KiB for a decoded
message body, subject to stricter channel limits. Control payloads have a smaller
512-byte cap. Metadata overhead still counts toward the 4,096-byte total.
The first filesystem profile has no fragmentation or external attachment fetch.
If a message does not fit, return `payload_too_large`.

Readers require an exact filename pattern and regular file. Read at most
4,097 bytes, reject shorter/longer records, verify the filename hash, then parse
under depth/string/count limits. Reject duplicate keys, unknown fields,
unsupported versions, invalid UTF-8, invalid padding and invalid identifiers.
Canonical JSON gives signatures one representation; a future binary header can
have its own negotiated version if measurement justifies it. Peers never patch
byte offsets in a card already published.

A hash detects corruption and gives content addressing. It does not establish
the sender's identity, consent or confidentiality.

## A small vocabulary

| Kind | Meaning |
|---|---|
| `offer` | Advertise an expiring purpose/audience within existing host consent |
| `join_request` | Ask the coordinator to join; this is the lobby ACK |
| `accepted` / `rejected` | Authoritative result bound to the original request |
| `publish` | Submit one bounded message under a stable publication ID |
| `receipt` | Report committed/rejected/uncertain publication and sequence |
| `pull` / `delivery` | Request/return bounded messages after durable ACK |
| `ack` | Acknowledge an offered contiguous sequence range explicitly |
| `status` / `revoked` / `closed` | Report authority state or terminate admission |

The handshake is `offer -> join_request -> accepted/rejected -> active`.
Seeing a card, adding a signature or receiving a join request never activates
membership. Acceptance requires current user/standing consent, authenticated
identities, matching purpose/audience, valid session generations and an unexpired
request. AAP can authorize automatic pairing within a standing cooperation grant;
agents cannot create or broaden that grant.

Group and private channels have independent memberships. Multiple routes may
carry the same request ID: the coordinator deduplicates globally, so a fallback
does not create another publication. Changed content under an existing operation
ID conflicts. Revoked/expired offers and receipts cannot reopen a channel.

## Publishing without shared-file races

Each identity has one enforced writer outbox. Peers create new records instead of
editing a common mutable file. A TC writes a freshly and exclusively created
temporary file in its own outbox, synchronizes and closes it, then publishes under
the hash filename in the same directory. Existing immutable destinations must
never be replaced with different content. An already present, verified identical
record is a retry; a publication conflict or an unverifiable destination is an
explicit error.

The mount profile must support publication that exposes only a complete old or
new record. Atomic visibility and persistence are distinct. File synchronization,
directory publication persistence and cross-client coherence need platform-specific
verification; a 4 KiB write alone is not an atomicity guarantee.
[Rust documents filesystem-dependent rename behavior](https://doc.rust-lang.org/stable/std/fs/fn.rename.html)
and [explicit file synchronization](https://doc.rust-lang.org/stable/std/fs/struct.File.html#method.sync_all).

Poll under a bounded record count/time budget, including on startup and reconnect.
Notifications can wake a poller sooner but never carry authoritative delivery.
TC's existing file probe already selects polling for WSL 9P/DrvFS and includes a
notification safety interval; it is not already a durable directory mailbox.

Persist the pending operation before publication. The coordinator persists its
result transactionally before producing a response card. If publication or the
reply is lost, retry/reconcile under the original ID. A restart rescans bounded
outstanding records and reconstructs locally persisted outcomes.

Receiving or reading a file is not ACK. Explicit ACK follows application delivery
and persists locally/coordinator-side before collection. Writers collect their
own records only under ACK/expiry and receipt-retention rules. Maintain tombstones
or reject expired producer generations so late retries cannot recreate expired
publications. Return exact gaps if approved expiry removes unread data.

Use a **six-record sliding window per direction** for message traffic. Up to six
immutable 4 KiB records can be in flight, each carrying its operation/publication
ID and sequence. An ACK frees capacity; no live slot is overwritten. This permits
pipelining while bounding message-card bytes to 24 KiB per direction, separate
from bounded control traffic and local retained-message budgets. Sequence and
hash linkage make missing/out-of-order records explicit. Retries retain their
IDs; cumulative ACK never skips an undispatched gap. Larger messages still must
fit one bounded record in this first profile.

Reserve a separate small allowance for ACK, rejection, revoke and health records
so a full data window can make progress. Announce effective window/record limits
at pairing; a peer may lower them. Do not promise a bandwidth multiplier: actual
throughput depends on mount coherence, polling delay, compression and ACK latency.

## Signatures and an ordered ledger

Use a maintained implementation of a standard signature algorithm, proposed
Ed25519, and a host-approved public-key registry. Private signing material remains
outside model context, repository files and mailboxes. A self-signed unknown key
cannot join; the host must bind its public key to the principal/session and consent.
[Ed25519 is specified in RFC 8032](https://www.rfc-editor.org/rfc/rfc8032.html).

Sign a domain-separated canonical envelope excluding only its signature value
and padding. Include version, kind, origin, recipient, operation/channel ID,
generation, consent revision, expiry, codec, decoded length and the exact encoded
payload. Verify authenticity before decompressing or processing that payload.
[RFC 8785 defines JSON canonicalization](https://www.rfc-editor.org/rfc/rfc8785.html).
Use decimal strings for unbounded sequence numbers and timestamps; reject floats
where the schema requires exact integers. Do not design custom cryptography.

For every committed authority event, store channel/coordinator generation,
monotonic sequence, coordinator UTC observation time, operation ID, content hash,
previous ledger-entry hash, consent revision and coordinator signature. This
ledger records acceptance, publication, membership change, ACK and revocation.
Sender timestamps are separate claims; sequence is the ordering authority.
Clock rollback/skew does not reorder committed events or authorize expired peers.
Use bounded lifetime policy and monotonic timers for live waits.

A signature proves control of an approved key; it does not prove that a clock was
accurate. A hash chain makes tampering evident relative to a retained trusted head;
it alone cannot detect truncation or rollback of an entire store. Peers/host retain
signed checkpoints outside the authority's rollback domain. Restore/key rotation
needs an explicit host-approved authority generation and checkpoint relationship;
never silently restart at sequence one under the old identity.

Bound ledger retention through signed segment checkpoints and retained operation
receipts. Expiry/compaction must disclose removed ranges. There is one authoritative
sequence per coordinator/channel, not a fabricated total order between independent
coordinators. No mining, voting or distributed-consensus service is required.

## Compression with hard decode bounds

`none` is mandatory. Negotiate an optional standard Zstandard body codec when
both endpoints advertise support; pairing/consent/control records remain
uncompressed. Proposed compression runs at a bounded low level, has no external
dictionary, and is used only when the complete record becomes smaller.
Compressed bytes use base64 in the JSON envelope, whose expansion counts toward
the fixed file size. Compression happens before any private-payload encryption.
[RFC 8878 specifies the Zstandard format](https://www.rfc-editor.org/rfc/rfc8878.html).

Check the signed declared decoded length against the negotiated cap before
allocating. Stream into a bounded destination and reject actual excess, declared
length mismatch, unsupported codecs, dictionary requests, oversized decoder
windows, trailing/concatenated frames and incomplete streams. Give decoding a
bounded memory/work budget; suspend repeat offenders by authenticated principal.
Compression never bypasses byte, message, storage, rate or tokenizer limits.
Strict configured token limits apply to decoded text using a pinned supported
tokenizer; unavailable exact counting is an explicit refusal.

The current TC workspace has no communication compression/tokenizer implementation.
Dependency selection, measured CPU cost and cross-platform builds are required
before this proposed codec is advertised.

## Permissions, privacy and flooding

Host configuration pins the rendezvous root, coordinator identity, key registry,
audience, retention and quotas. Reject symlinks/reparse points, special files,
unexpected subdirectories and root escapes. Avoid checking a canonical path and
then reopening it through attacker-replaceable directories: use safe platform
handles or require a protected trusted-writer root.

Proposed per-principal total pending allowance is 128 records / 512 KiB of card bytes,
including the six-record per-direction data windows and bounded control records,
plus per-root/channel byte, count and rate caps. Directory enumeration, rejected
records, temporary files and diagnostics also need bounds. Stop accepting a
principal after repeated violations; reconnects/new names cannot reset its budget.
Reserve control capacity for revocation/health and healthy participants.

Application quotas constrain cooperating clients. A process with unrestricted
write access can create unlimited junk or erase other outboxes. Host filesystem
permissions/quotas or isolation must enforce that boundary. Refuse a protected
mode when the host cannot provide it; do not claim immunity from a shared-owner
process.

Signatures and opaque filenames do not encrypt messages. An all-readable checkout
cannot provide private back channels through application ACLs alone. Private
filesystem delivery needs distinct enforced mounts/permissions or an approved
authenticated-encryption adapter with host-managed keys. The first filesystem
implementation may expose only explicitly approved shared audiences until that
privacy boundary is verified.

## Gates before support

Exercise two real TCs on each supported mount profile, including Windows/WSL
mapping when claimed. Prove bounded publication visibility, reconnect rescans,
lost/duplicate replies, restart reconciliation, explicit ACK, crash during publish,
stale/revoked generations, conflicting IDs, consent revocation and truthful quota/
disk failures. Inject oversized/partial records, forged signatures, malformed
JSON, decompression bombs, clock rollback, symlink/reparse replacement and floods.
Verify a healthy peer and owner control remain usable during rejected traffic.

Mark unsupported atomicity, durability, privacy or permission enforcement
explicitly. Cloud-sync folders, SMB/NFS/DrvFS/virtiofs mounts and a shared writable
raw disk are not interchangeable guarantees. Sharing one raw filesystem image
read-write between independent kernels is outside this protocol.

These are proposed requirements, not tested features in the current embed repair.
