# Fourier features for the surrogate model

Created: 2026-09-19 14:05
ID: R002

## Context

The MLP surrogate underfits high-frequency modes of the solution.

## Investigation

Random Fourier features with sigma = 10 recover the spectrum up to k = 32.
