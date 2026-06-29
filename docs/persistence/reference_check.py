#!/usr/bin/env python3
"""
Executable reference for the alpha/gamma Galois connection.

This is NOT the implementation — it is a check that the spec's laws hold on
concrete examples, so the Rust implementation has an oracle to match. Uses
lxml for C14N (W3C canonicalisation), which is the same canonical form the
Rust implementation must produce.
"""
from lxml import etree
import hashlib
import copy

BASE = "https://simulboot.dev/session/v1"
COEFF = "https://simulboot.dev/session/v1/coefficients"

def parse(s):
    return etree.fromstring(s.encode())

def c14n(elem):
    """W3C exclusive canonicalisation of an element (bytes)."""
    return etree.tostring(elem, method="c14n2")

def base_projection(doc):
    """base(doc): copy with all coefficients-namespace elements removed."""
    d = copy.deepcopy(doc)
    for el in d.iter():
        # remove children that are in the COEFF namespace
        for child in list(el):
            if isinstance(child.tag, str) and child.tag.startswith("{" + COEFF + "}"):
                el.remove(child)
    return d

def alpha(doc):
    """alpha : I1 -> I0. Delete the coefficients-namespace subtree."""
    return base_projection(doc)

def gamma(doc):
    """gamma : I0 -> I1. Insert a top-valued <coefficients> block."""
    d = copy.deepcopy(doc)
    # find all surface ids in base namespace
    surfaces = d.findall(f"{{{BASE}}}surfaces/{{{BASE}}}surface")
    coeffs = etree.SubElement(d, f"{{{COEFF}}}coefficients")
    coeffs.set("xmlns", COEFF)  # nominal; lxml handles ns via Clark notation
    for s in surfaces:
        sid = s.get("id")
        sc = etree.SubElement(coeffs, f"{{{COEFF}}}surface-coefficient")
        sc.set("surface-ref", sid)
        timing = etree.SubElement(sc, f"{{{COEFF}}}timing")
        dp = etree.SubElement(timing, f"{{{COEFF}}}delay-path")
        dp.set("lo", "-INF"); dp.set("hi", "+INF")           # timing TOP
        sec = etree.SubElement(sc, f"{{{COEFF}}}security")
        sec.set("confidentiality", "public")                  # security TOP
        sec.set("integrity", "untrusted")
        lin = etree.SubElement(sc, f"{{{COEFF}}}linearity")
        lin.set("count", "omega")                             # linearity TOP
    return d

def session_id(doc):
    """SHA256(C14N(base(doc))) — identity is base-determined."""
    return hashlib.sha256(c14n(base_projection(doc))).hexdigest()

def coeff_is_top(doc):
    """check every surface-coefficient is at semiring top."""
    scs = doc.findall(f"{{{COEFF}}}coefficients/{{{COEFF}}}surface-coefficient")
    for sc in scs:
        dp = sc.find(f"{{{COEFF}}}timing/{{{COEFF}}}delay-path")
        if dp.get("lo") != "-INF" or dp.get("hi") != "+INF":
            return False
        sec = sc.find(f"{{{COEFF}}}security")
        if sec.get("confidentiality") != "public" or sec.get("integrity") != "untrusted":
            return False
        lin = sc.find(f"{{{COEFF}}}linearity")
        if lin.get("count") != "omega":
            return False
    return True

# ---- a concrete v0 image ----
V0 = """<session xmlns="https://simulboot.dev/session/v1"
   id="sha256:PLACEHOLDER" created="2026-06-28T19:44:09Z" schema-version="1">
  <surfaces>
    <surface id="sha256:aaa" name="macOS" order="0">
      <host><address>127.0.0.1:7001</address><os>macOS</os>
        <machine>Aleth-MacBook</machine><capture>window:Safari</capture></host>
      <codec>H265</codec><dimensions width="960" height="540"/>
    </surface>
    <surface id="sha256:bbb" name="Windows" order="1">
      <host><address>100.1.1.2:7001</address><os>Windows</os>
        <machine>Aleth-PC</machine><capture>display:0</capture></host>
      <codec>H265</codec><dimensions width="960" height="540"/>
    </surface>
  </surfaces>
  <layout><strip scroll-pos="0.0"/><focus surface-ref="sha256:aaa"/></layout>
</session>"""

def run():
    img0 = parse(V0)
    print("=== L1: alpha(gamma(img0)) == img0  (C14N-identical) ===")
    lhs = c14n(alpha(gamma(img0)))
    rhs = c14n(img0)
    print("L1 holds:", lhs == rhs)

    print("\n=== identity-stability: session_id(gamma(img0)) == session_id(img0) ===")
    print("holds:", session_id(gamma(img0)) == session_id(img0))

    print("\n=== L2-deflation: gamma(alpha(img1)) has base==img1.base and coeff==TOP ===")
    img1 = gamma(img0)  # a v1 image (here, top-valued; suffices for structure)
    ga = gamma(alpha(img1))
    base_eq = c14n(base_projection(ga)) == c14n(base_projection(img1))
    print("base preserved:", base_eq, " coeff==TOP:", coeff_is_top(ga))

    print("\n=== L2-equality-case: img1 already top => gamma(alpha(img1)) == img1 ===")
    print("holds:", c14n(gamma(alpha(img1))) == c14n(img1))

    print("\n=== L3 (with identity-stub renderer): alpha(ckpt(load(gamma(img0)))) == img0 ===")
    load = lambda x: x          # stub load_v1
    ckpt = lambda x: x          # stub checkpoint_v1 (preserves coefficients)
    out = c14n(alpha(ckpt(load(gamma(img0)))))
    print("holds:", out == c14n(img0))

if __name__ == "__main__":
    run()
