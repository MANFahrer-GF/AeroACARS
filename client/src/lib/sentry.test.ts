// v0.19.x FIX: beforeBreadcrumb only stripped query strings from fetch/xhr
// breadcrumbs. Sentry's browser history integration also emits `navigation`
// breadcrumbs with data.from/data.to URLs — ipc.ts's consumePinFromUrl()
// strips a `?pin=NNNNNN` LAN-pairing PIN from the address bar via
// history.replaceState, and that PIN was still in the URL at the instant of
// navigation, reachable verbatim through this uncovered breadcrumb path.

import { describe, it, expect } from "vitest";
import { redactBreadcrumb } from "./sentry";
import type { Breadcrumb } from "@sentry/react";

describe("redactBreadcrumb", () => {
  it("strips the query string from a navigation breadcrumb's `to` URL", () => {
    const crumb: Breadcrumb = {
      category: "navigation",
      data: { from: "/", to: "/?pin=123456" },
    };
    const out = redactBreadcrumb(crumb);
    expect(out.data?.to).toBe("/");
    expect(out.data?.to).not.toContain("123456");
  });

  it("strips the query string from a navigation breadcrumb's `from` URL", () => {
    const crumb: Breadcrumb = {
      category: "navigation",
      data: { from: "/?pin=654321", to: "/settings" },
    };
    const out = redactBreadcrumb(crumb);
    expect(out.data?.from).toBe("/");
    expect(out.data?.from).not.toContain("654321");
  });

  it("still strips fetch/xhr breadcrumb URLs (existing behavior, unchanged)", () => {
    const crumb: Breadcrumb = {
      category: "fetch",
      data: { url: "https://va.example.com/api/pireps?token=abc" },
    };
    const out = redactBreadcrumb(crumb);
    expect(out.data?.url).toBe("https://va.example.com/api/pireps");
  });

  it("leaves other breadcrumb categories untouched", () => {
    const crumb: Breadcrumb = {
      category: "ui.click",
      message: "clicked button",
    };
    const out = redactBreadcrumb(crumb);
    expect(out).toEqual(crumb);
  });

  it("does not throw on a navigation breadcrumb with no data", () => {
    const crumb: Breadcrumb = { category: "navigation" };
    expect(() => redactBreadcrumb(crumb)).not.toThrow();
  });
});
