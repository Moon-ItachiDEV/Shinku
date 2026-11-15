//! Catalogue des règles de fichiers inutiles (droits de l'utilisateur).
//!
//! Tout ce qui figure ici se recrée seul : caches, fichiers temporaires,
//! journaux, rapports d'erreur. Ajouter une règle, c'est affirmer que
//! supprimer cet emplacement ne fait perdre aucune donnée à personne.
//!
//! Ce qui est volontairement absent :
//!
//! - `LocalCache`, `LocalState` et `Settings` des apps du Store : malgré leur
//!   nom, ils contiennent les données de l'app (les messages de WhatsApp, par
//!   exemple) ;
//! - le stockage hors connexion des sites (`Service Worker`, `IndexedDB`) et
//!   la musique téléchargée par Spotify : ce ne sont pas des caches ;
//! - `Package Cache` et `Downloaded Installations` : Windows s'en sert pour
//!   réparer ou désinstaller un programme ;
//! - les caches de shaders des cartes graphiques, gérés par le mode jeu avec
//!   son propre seuil ;
//! - tout ce qui demande les droits administrateur (`C:\Windows\Temp`,
//!   téléchargements de Windows Update…), qui arrivera avec le helper.

use super::JunkCategory::{AppCaches, LogsAndReports, TempFiles, UpdateLeftovers};
use super::{RootCheck, Rule, SPARE_RECENT_DAYS};
use crate::platform::KnownFolder::{LocalAppData, RoamingAppData, Temp};

const CHROME: &[&str] = &["chrome.exe"];
const EDGE: &[&str] = &["msedge.exe"];
const BRAVE: &[&str] = &["brave.exe"];
const VIVALDI: &[&str] = &["vivaldi.exe"];
const OPERA: &[&str] = &["opera.exe"];
const FIREFOX: &[&str] = &["firefox.exe"];
const DISCORD: &[&str] = &["Discord.exe"];
const SLACK: &[&str] = &["slack.exe"];
const VS_CODE: &[&str] = &["Code.exe"];
const TEAMS: &[&str] = &["Teams.exe"];
const STEAM: &[&str] = &["steamwebhelper.exe"];

/// Les règles, dans l'ordre où elles revendiquent un dossier : si deux règles
/// désignent le même emplacement, la première l'emporte.
pub static CATALOG: &[Rule] = &[
    // ── Caches des apps ────────────────────────────────────────────────
    // Navigateurs de la famille Chromium : un dossier par profil
    // (`Default`, `Profile 1`…), et dedans trois caches qui se recréent.
    Rule::new(
        "edge.cache",
        AppCaches,
        "Microsoft Edge",
        LocalAppData,
        &["Microsoft", "Edge", "User Data", "*", "Cache"],
    )
    .close(EDGE),
    Rule::new(
        "edge.code-cache",
        AppCaches,
        "Microsoft Edge",
        LocalAppData,
        &["Microsoft", "Edge", "User Data", "*", "Code Cache"],
    )
    .close(EDGE),
    Rule::new(
        "edge.gpu-cache",
        AppCaches,
        "Microsoft Edge",
        LocalAppData,
        &["Microsoft", "Edge", "User Data", "*", "GPUCache"],
    )
    .close(EDGE),
    Rule::new(
        "chrome.cache",
        AppCaches,
        "Google Chrome",
        LocalAppData,
        &["Google", "Chrome", "User Data", "*", "Cache"],
    )
    .close(CHROME),
    Rule::new(
        "chrome.code-cache",
        AppCaches,
        "Google Chrome",
        LocalAppData,
        &["Google", "Chrome", "User Data", "*", "Code Cache"],
    )
    .close(CHROME),
    Rule::new(
        "chrome.gpu-cache",
        AppCaches,
        "Google Chrome",
        LocalAppData,
        &["Google", "Chrome", "User Data", "*", "GPUCache"],
    )
    .close(CHROME),
    Rule::new(
        "brave.cache",
        AppCaches,
        "Brave",
        LocalAppData,
        &["BraveSoftware", "Brave-Browser", "User Data", "*", "Cache"],
    )
    .close(BRAVE),
    Rule::new(
        "brave.code-cache",
        AppCaches,
        "Brave",
        LocalAppData,
        &["BraveSoftware", "Brave-Browser", "User Data", "*", "Code Cache"],
    )
    .close(BRAVE),
    Rule::new(
        "brave.gpu-cache",
        AppCaches,
        "Brave",
        LocalAppData,
        &["BraveSoftware", "Brave-Browser", "User Data", "*", "GPUCache"],
    )
    .close(BRAVE),
    Rule::new(
        "vivaldi.cache",
        AppCaches,
        "Vivaldi",
        LocalAppData,
        &["Vivaldi", "User Data", "*", "Cache"],
    )
    .close(VIVALDI),
    Rule::new(
        "vivaldi.code-cache",
        AppCaches,
        "Vivaldi",
        LocalAppData,
        &["Vivaldi", "User Data", "*", "Code Cache"],
    )
    .close(VIVALDI),
    // Opera range son cache à part de son profil.
    Rule::new(
        "opera.cache",
        AppCaches,
        "Opera",
        LocalAppData,
        &["Opera Software", "Opera Stable", "Cache"],
    )
    .close(OPERA),
    Rule::new(
        "opera-gx.cache",
        AppCaches,
        "Opera GX",
        LocalAppData,
        &["Opera Software", "Opera GX Stable", "Cache"],
    )
    .close(OPERA),
    // Firefox : cache réseau et cache de démarrage de chaque profil.
    Rule::new(
        "firefox.cache",
        AppCaches,
        "Mozilla Firefox",
        LocalAppData,
        &["Mozilla", "Firefox", "Profiles", "*", "cache2"],
    )
    .close(FIREFOX),
    Rule::new(
        "firefox.startup-cache",
        AppCaches,
        "Mozilla Firefox",
        LocalAppData,
        &["Mozilla", "Firefox", "Profiles", "*", "startupCache"],
    )
    .close(FIREFOX),
    // Apps construites sur Chromium : mêmes trois caches, sous AppData\Roaming.
    Rule::new("discord.cache", AppCaches, "Discord", RoamingAppData, &["discord", "Cache"])
        .close(DISCORD),
    Rule::new(
        "discord.code-cache",
        AppCaches,
        "Discord",
        RoamingAppData,
        &["discord", "Code Cache"],
    )
    .close(DISCORD),
    Rule::new("discord.gpu-cache", AppCaches, "Discord", RoamingAppData, &["discord", "GPUCache"])
        .close(DISCORD),
    Rule::new("slack.cache", AppCaches, "Slack", RoamingAppData, &["Slack", "Cache"]).close(SLACK),
    Rule::new("slack.code-cache", AppCaches, "Slack", RoamingAppData, &["Slack", "Code Cache"])
        .close(SLACK),
    Rule::new(
        "teams.cache",
        AppCaches,
        "Microsoft Teams",
        RoamingAppData,
        &["Microsoft", "Teams", "Cache"],
    )
    .close(TEAMS),
    Rule::new(
        "teams.code-cache",
        AppCaches,
        "Microsoft Teams",
        RoamingAppData,
        &["Microsoft", "Teams", "Code Cache"],
    )
    .close(TEAMS),
    Rule::new("vscode.cache", AppCaches, "Visual Studio Code", RoamingAppData, &["Code", "Cache"])
        .close(VS_CODE),
    Rule::new(
        "vscode.cached-data",
        AppCaches,
        "Visual Studio Code",
        RoamingAppData,
        &["Code", "CachedData"],
    )
    .close(VS_CODE),
    Rule::new(
        "vscode.code-cache",
        AppCaches,
        "Visual Studio Code",
        RoamingAppData,
        &["Code", "Code Cache"],
    )
    .close(VS_CODE),
    Rule::new(
        "vscode.extension-downloads",
        AppCaches,
        "Visual Studio Code",
        RoamingAppData,
        &["Code", "CachedExtensionVSIXs"],
    )
    .close(VS_CODE),
    // Steam : le cache de son navigateur intégré, pas les jeux ni leurs données.
    Rule::new(
        "steam.web-cache",
        AppCaches,
        "Steam",
        LocalAppData,
        &["Steam", "htmlcache", "Cache"],
    )
    .close(STEAM),
    Rule::new(
        "steam.web-code-cache",
        AppCaches,
        "Steam",
        LocalAppData,
        &["Steam", "htmlcache", "Code Cache"],
    )
    .close(STEAM),
    // Windows : miniatures et icônes de l'Explorateur, que Windows propose
    // lui-même de vider dans son outil de nettoyage.
    Rule::new(
        "windows.thumbnails",
        AppCaches,
        "Miniatures de l'Explorateur",
        LocalAppData,
        &["Microsoft", "Windows", "Explorer"],
    )
    .files(&["thumbcache_*.db", "iconcache_*.db"]),
    // Cache web de Windows. On y épargne le récent : Outlook y dépose les
    // pièces jointes qu'on vient d'ouvrir.
    Rule::new(
        "windows.inet-cache",
        AppCaches,
        "Cache web de Windows",
        LocalAppData,
        &["Microsoft", "Windows", "INetCache"],
    )
    .older_than(SPARE_RECENT_DAYS),
    // Apps du Store : chacune a son cache web.
    Rule::new(
        "store-apps.inet-cache",
        AppCaches,
        "Apps du Microsoft Store",
        LocalAppData,
        &["Packages", "*", "AC", "INetCache"],
    )
    .older_than(SPARE_RECENT_DAYS),
    // ── Fichiers temporaires ───────────────────────────────────────────
    // `%TEMP%`. Chaque élément est jugé en bloc : le dossier d'un installeur
    // en cours n'est pas vidé à moitié.
    Rule::new("windows.temp", TempFiles, "Fichiers temporaires de Windows", Temp, &[])
        .entries_older_than(SPARE_RECENT_DAYS)
        .check(RootCheck::TempFolder),
    // Son emplacement habituel, au cas où `%TEMP%` aurait été déplacé.
    Rule::new(
        "windows.temp-default",
        TempFiles,
        "Fichiers temporaires de Windows",
        LocalAppData,
        &["Temp"],
    )
    .entries_older_than(SPARE_RECENT_DAYS),
    Rule::new(
        "store-apps.temp",
        TempFiles,
        "Apps du Microsoft Store",
        LocalAppData,
        &["Packages", "*", "AC", "Temp"],
    )
    .entries_older_than(SPARE_RECENT_DAYS),
    Rule::new(
        "store-apps.temp-state",
        TempFiles,
        "Apps du Microsoft Store",
        LocalAppData,
        &["Packages", "*", "TempState"],
    )
    .entries_older_than(SPARE_RECENT_DAYS),
    // ── Restes de mises à jour ─────────────────────────────────────────
    // Fichiers de travail du programme de mise à jour de nombreuses apps
    // (Discord, Slack, Teams…). Les restes de Windows Update demandent les
    // droits administrateur : ils viendront avec le helper.
    Rule::new(
        "updaters.squirrel-temp",
        UpdateLeftovers,
        "Mises à jour des apps",
        LocalAppData,
        &["SquirrelTemp"],
    )
    .older_than(SPARE_RECENT_DAYS),
    // ── Journaux et rapports d'erreur ──────────────────────────────────
    Rule::new(
        "windows.crash-dumps",
        LogsAndReports,
        "Rapports de plantage des apps",
        LocalAppData,
        &["CrashDumps"],
    )
    .files(&["*.dmp"])
    .older_than(SPARE_RECENT_DAYS),
    Rule::new(
        "windows.error-reports-archive",
        LogsAndReports,
        "Rapports d'erreur de Windows",
        LocalAppData,
        &["Microsoft", "Windows", "WER", "ReportArchive"],
    )
    .entries_older_than(SPARE_RECENT_DAYS),
    Rule::new(
        "windows.error-reports-queue",
        LogsAndReports,
        "Rapports d'erreur de Windows",
        LocalAppData,
        &["Microsoft", "Windows", "WER", "ReportQueue"],
    )
    .entries_older_than(SPARE_RECENT_DAYS),
    Rule::new(
        "edge.crash-reports",
        LogsAndReports,
        "Microsoft Edge",
        LocalAppData,
        &["Microsoft", "Edge", "User Data", "Crashpad", "reports"],
    )
    .files(&["*.dmp"])
    .older_than(SPARE_RECENT_DAYS),
    Rule::new(
        "chrome.crash-reports",
        LogsAndReports,
        "Google Chrome",
        LocalAppData,
        &["Google", "Chrome", "User Data", "Crashpad", "reports"],
    )
    .files(&["*.dmp"])
    .older_than(SPARE_RECENT_DAYS),
    Rule::new(
        "vscode.logs",
        LogsAndReports,
        "Visual Studio Code",
        RoamingAppData,
        &["Code", "logs"],
    )
    .entries_older_than(SPARE_RECENT_DAYS),
];
