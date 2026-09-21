# Boundary condition leaks energy

Created: 2026-09-18 09:30
ID: R001

## Context

Total energy drifts upward when the Neumann boundary is active.

## Investigation

The ghost-cell update runs before the flux correction.
