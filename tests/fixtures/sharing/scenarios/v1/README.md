# Transfer scenario schema v1

These project-authored scenarios encode the confirmed connection and transfer
seams. They are not generated protocol fixtures and contain no copied upstream
data.

Each driver must perform the named direction, connection role, medium, peer
decision, and cancellation through a public seam. It reports only terminal
outcome, transferred byte count, and cleanup state. Fast fakes and admitted
simulator or oracle adapters consume these same files.

Passing the fast fake proves that a scenario is valid and deterministic. It is
not evidence that a medium works. A medium is trusted only after its real,
virtualized, or oracle-backed adapter passes the same scenario contract.
