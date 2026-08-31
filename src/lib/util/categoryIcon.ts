/**
 * Lucide icon-name → Svelte component binding for the category icons.
 *
 * The icon CHOICE is data-driven: each category's label + Lucide icon name live
 * in `src-tauri/data/agency-categories.json` (read by `corpus/mod.rs::category_meta`).
 * This UI does not re-decide icons in code.
 *
 * This map is NOT a second source of truth — it's purely the bundler binding
 * from the data-provided Lucide name to its tree-shakeable Svelte component.
 * Lucide-svelte has no render-by-name without bundling all ~1600 icons, so the
 * components we use must be statically imported here.
 *
 * Adding a category? Set its Lucide `icon` name in `agency-categories.json`,
 * then add the one matching Lucide import below. Unknown names fall back to
 * `HelpCircle` so a missing import never crashes — but it WILL look out of
 * place, so keep this in sync with the taxonomy.
 */

import type { Component } from "svelte";

import Brain from "@lucide/svelte/icons/brain";
import BadgeCheck from "@lucide/svelte/icons/badge-check";
import BadgeDollarSign from "@lucide/svelte/icons/badge-dollar-sign";
import BookOpen from "@lucide/svelte/icons/book-open";
import Briefcase from "@lucide/svelte/icons/briefcase";
import BriefcaseBusiness from "@lucide/svelte/icons/briefcase-business";
import Cloud from "@lucide/svelte/icons/cloud";
import CloudCog from "@lucide/svelte/icons/cloud-cog";
import Code from "@lucide/svelte/icons/code";
import Cpu from "@lucide/svelte/icons/cpu";
import Database from "@lucide/svelte/icons/database";
import FileCode from "@lucide/svelte/icons/file-code";
import FileText from "@lucide/svelte/icons/file-text";
import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
import Globe from "@lucide/svelte/icons/globe";
import GraduationCap from "@lucide/svelte/icons/graduation-cap";
import Glasses from "@lucide/svelte/icons/glasses";
import HeartPulse from "@lucide/svelte/icons/heart-pulse";
import HelpCircle from "@lucide/svelte/icons/help-circle";
import Lock from "@lucide/svelte/icons/lock";
import MessageSquare from "@lucide/svelte/icons/message-square";
import Music from "@lucide/svelte/icons/music";
import Palette from "@lucide/svelte/icons/palette";
import PanelsTopLeft from "@lucide/svelte/icons/panels-top-left";
import PenTool from "@lucide/svelte/icons/pen-tool";
import Settings from "@lucide/svelte/icons/settings";
import RadioTower from "@lucide/svelte/icons/radio-tower";
import Shapes from "@lucide/svelte/icons/shapes";
import Terminal from "@lucide/svelte/icons/terminal";
import Video from "@lucide/svelte/icons/video";

// Agency Agents category icons (the 16 agent categories — agency-categories.json).
import Box from "@lucide/svelte/icons/box";
import Boxes from "@lucide/svelte/icons/boxes";
import ClipboardList from "@lucide/svelte/icons/clipboard-list";
import DollarSign from "@lucide/svelte/icons/dollar-sign";
import FlaskConical from "@lucide/svelte/icons/flask-conical";
import LifeBuoy from "@lucide/svelte/icons/life-buoy";
import Map from "@lucide/svelte/icons/map";
import Megaphone from "@lucide/svelte/icons/megaphone";
import Network from "@lucide/svelte/icons/network";
import Workflow from "@lucide/svelte/icons/workflow";
import ShieldCheck from "@lucide/svelte/icons/shield-check";
import Sparkles from "@lucide/svelte/icons/sparkles";
import Target from "@lucide/svelte/icons/target";
import TrendingUp from "@lucide/svelte/icons/trending-up";

const ICONS: Record<string, Component> = {
  BadgeCheck,
  BadgeDollarSign,
  BookOpen,
  Brain,
  Briefcase,
  BriefcaseBusiness,
  Cloud,
  CloudCog,
  Code,
  Cpu,
  Database,
  FileCode,
  FileText,
  Gamepad2,
  Glasses,
  Globe,
  GraduationCap,
  HelpCircle,
  HeartPulse,
  Lock,
  MessageSquare,
  Music,
  Palette,
  PanelsTopLeft,
  PenTool,
  Settings,
  RadioTower,
  Shapes,
  Terminal,
  Video,
  // Agency categories
  Box,
  Boxes,
  ClipboardList,
  DollarSign,
  FlaskConical,
  LifeBuoy,
  Map,
  Megaphone,
  Network,
  ShieldCheck,
  Workflow,
  Sparkles,
  Target,
  TrendingUp,
};

/**
 * Resolve a Lucide icon by PascalCase name. Falls back to `HelpCircle` for
 * any unknown name — see module docstring for why that fallback exists.
 */
export function resolveCategoryIcon(name: string): Component {
  return ICONS[name] ?? HelpCircle;
}
