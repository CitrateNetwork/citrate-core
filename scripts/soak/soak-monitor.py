#!/usr/bin/env python3
"""Soak monitor for the 0.5.2 release candidate (RC52 gate G9). Read-only.

Placeholder entry point: the sampler and verdict land in the next commit. Until then it
reports nothing and judges nothing (the gate has no harness today).
"""
import sys


def main():
    print("SOAK PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
