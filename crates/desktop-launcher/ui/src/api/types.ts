// Generated from the launcher's Rust types by `cargo test -p desktop-launcher contract`
// with CINNABAR_WRITE_UI_TYPES=1; do not edit by hand.

export type Channel = "beta" | "nightly" | "stable";

export type Platform = "macos" | "windows" | "linux";

export type InstallState = { version: string, 
/**
 * The publication number, or `dev` for an unpublished build.
 */
build: string, 
/**
 * The desired channel.
 */
channel: Channel, 
/**
 * The channel the installed payload was published on.
 */
installedChannel: Channel, 
/**
 * Only channels with a publishing track.
 */
publishedChannels: Array<Channel>, launcherVersion: string, 
/**
 * Lets the UI leave room for native window controls.
 */
platform: Platform, 
/**
 * The command surface this launcher offers a web UI bundle.
 */
nativeApiVersion: number, };

export type UpdateStage = "check" | "download" | "verify" | "install";

export type UpdateState = { "kind": "idle", lastChecked?: string, } | { "kind": "checking" } | { "kind": "upToDate", lastChecked: string, } | { "kind": "available", version: string, channel: Channel, notesUrl?: string, sizeBytes?: number, 
/**
 * A server refused this build as outdated.
 */
required: boolean, 
/**
 * The selected channel's release is older than the installed build.
 */
downgrade: boolean, } | { "kind": "downloading", version: string, receivedBytes: number, totalBytes?: number, } | { "kind": "verifying", version: string, } | { "kind": "installing", version: string, } | { "kind": "unavailable", reason: string, } | { "kind": "failed", message: string, 
/**
 * The step that failed, so a retry repeats it.
 */
stage: UpdateStage, retryable: boolean, canPlayInstalled: boolean, };

export type LastExit = { code: number | null, crashed: boolean, };

export type GameState = { "kind": "stopped", lastExit?: LastExit, } | { "kind": "starting" } | { "kind": "running", startedAt: string, };

export type NewsItem = { id: string, title: string, summary: string, url: string, imageUrl?: string, publishedAt: string, tag?: string, };

export type News = { items: Array<NewsItem>, stale: boolean, fetchedAt?: string, };

export type FeaturedServer = { id: string, name: string, 
/**
 * A host name, or an experience ID joined with port 0.
 */
address: string, port: number, description?: string, imageUrl?: string, online?: number, motd?: string, tags?: Array<string>, 
/**
 * Round trip in milliseconds; the launcher does not measure it yet.
 */
pingMs?: number, };

export type RecentServer = { name: string, address: string, port: number, imageUrl?: string, lastPlayedAt: string, };

export type Changelog = { version: string, publishedAt: string, highlights: Array<string>, url?: string, 
/**
 * True on the first launch of this version; reading it marks it seen.
 */
unseen: boolean, };

export type ServerTarget = { address: string, port: number, };

export type LaunchOptions = { server?: ServerTarget, };

export const PRODUCT_NAME = "Cinnabar";
export const SOURCE_URL = "https://github.com/bedrock-mc/cinnabar";
export const DISCORD_URL = "https://discord.gg/MeEz7BEHcM";
export const LICENSE_NAME = "PolyForm Noncommercial License 1.0.0";

export type { Api } from './api';
