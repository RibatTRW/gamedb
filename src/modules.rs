//! Module designation: which named system a file belongs to. Derived entirely
//! from existing structure — the file's own name when it is a root-namespace
//! type, otherwise the namespace it declares. First matching rule wins, so the
//! mapping is deterministic and every file lands in exactly one module.

use crate::parse::find_namespace;
use std::path::Path;

pub const STATE_UNIMPLEMENTED: &str = "UNIMPLEMENTED";
pub const STATE_PARTIAL: &str = "PARTIALLY_IMPLEMENTED";
pub const STATE_IMPLEMENTED: &str = "IMPLEMENTED";
pub const MODULE_STATES: [&str; 3] = [STATE_UNIMPLEMENTED, STATE_PARTIAL, STATE_IMPLEMENTED];

/// Rule keys: `b:` basename, `n:` namespace prefix, `x:` exact namespace.
/// Order is load-bearing. This is data, not code — edit it freely.
pub const MODULE_RULES: [(&str, &str, &str); 80] = [
    ("b:Player.cs", "core.player", "Player state & lifecycle"),
    ("b:Main.cs", "core.main", "World state & game loop"),
    ("b:NPC.cs", "core.npc", "NPC state & behaviour"),
    (
        "b:Projectile.cs",
        "core.projectile",
        "Projectile state & behaviour",
    ),
    ("b:WorldGen.cs", "core.worldgen", "World generation"),
    ("b:Item.cs", "core.item", "Item state"),
    ("b:Recipe.cs", "core.recipe", "Crafting"),
    ("b:Mount.cs", "core.mount", "Mounts & vehicles"),
    ("b:Tile.cs", "core.tile", "Tile state"),
    ("b:Chest.cs", "core.chest", "Container state"),
    ("b:Wiring.cs", "core.wiring", "Tile wiring & traps"),
    ("b:Collision.cs", "core.collision", "Collision detection"),
    ("b:Utils.cs", "core.support", "Shared runtime support"),
    (
        "b:DelegateMethods.cs",
        "core.support",
        "Shared runtime support",
    ),
    (
        "n:Terraria.IO",
        "io.persistence",
        "Save/load & file persistence",
    ),
    ("n:Terraria.Net", "io.netplay", "Multiplayer transport"),
    ("n:Terraria.Server", "io.server", "Server authority"),
    ("b:NetMessage.cs", "io.netplay", "Multiplayer transport"),
    ("b:Netplay.cs", "io.netplay", "Multiplayer transport"),
    ("b:MessageBuffer.cs", "io.netplay", "Multiplayer transport"),
    ("b:RemoteClient.cs", "io.netplay", "Multiplayer transport"),
    ("b:RemoteServer.cs", "io.netplay", "Multiplayer transport"),
    ("n:Terraria.Map", "io.map", "Map surface & minimap"),
    ("b:MapRenderer.cs", "io.map", "Map surface & minimap"),
    ("b:IngameOptions.cs", "config.options", "Runtime options"),
    ("b:GetItemSettings.cs", "config.options", "Runtime options"),
    (
        "b:TimeLogger.cs",
        "diag.logging",
        "Frame timing & scene metrics",
    ),
    (
        "b:SceneMetrics.cs",
        "diag.logging",
        "Frame timing & scene metrics",
    ),
    (
        "b:SceneState.cs",
        "diag.logging",
        "Frame timing & scene metrics",
    ),
    ("b:Program.cs", "entry.main", "Process entry point"),
    ("b:WindowsLaunch.cs", "entry.main", "Process entry point"),
    ("b:ScriptSandbox.cs", "entry.main", "Process entry point"),
    (
        "b:nativefiledialog.cs",
        "entry.platform",
        "Native platform shims",
    ),
    ("n:BCrypt", "entry.platform", "Native platform shims"),
    (
        "n:Terraria.GameContent.ItemDropRules",
        "domain.loot",
        "Loot tables",
    ),
    (
        "n:Terraria.GameContent.FishDropRules",
        "domain.loot",
        "Loot tables",
    ),
    (
        "n:Terraria.GameContent.Bestiary",
        "domain.bestiary",
        "Enemy bestiary & spawn rules",
    ),
    (
        "n:Terraria.GameContent.Generation",
        "domain.dungeon",
        "Dungeon generation",
    ),
    (
        "n:Terraria.WorldBuilding",
        "domain.dungeon",
        "Dungeon generation",
    ),
    (
        "n:Terraria.GameContent.Biomes",
        "domain.biomes",
        "Biomes & ambience",
    ),
    (
        "n:Terraria.GameContent.Skies",
        "domain.biomes",
        "Biomes & ambience",
    ),
    (
        "n:Terraria.GameContent.Drawing",
        "domain.drawing",
        "World painting & tile drawing",
    ),
    (
        "n:Terraria.GameContent.Animations",
        "domain.animation-content",
        "Content animation rigs",
    ),
    (
        "n:Terraria.GameContent.Tile_Entities",
        "domain.animation-content",
        "Content animation rigs",
    ),
    (
        "n:Terraria.GameContent.LeashedEntities",
        "domain.animation-content",
        "Content animation rigs",
    ),
    (
        "n:Terraria.GameContent.Events",
        "domain.events",
        "World events",
    ),
    (
        "n:Terraria.GameContent.Creative",
        "domain.creative",
        "Creative mode & journey",
    ),
    (
        "n:Terraria.GameContent.Golf",
        "domain.golf",
        "Golf minigame",
    ),
    (
        "n:Terraria.GameContent.Achievements",
        "domain.achievements",
        "Achievements",
    ),
    (
        "n:Terraria.Achievements",
        "domain.achievements",
        "Achievements",
    ),
    (
        "n:Terraria.GameContent.Items",
        "domain.items-content",
        "Item content definitions",
    ),
    (
        "n:Terraria.GameContent.Personalities",
        "domain.npc-content",
        "NPC content definitions",
    ),
    (
        "n:Terraria.GameContent.RGB",
        "domain.rgb",
        "Shader effect content",
    ),
    (
        "n:Terraria.GameContent.ObjectInteractions",
        "domain.interactions",
        "Object interactions",
    ),
    (
        "n:Terraria.ObjectData",
        "domain.interactions",
        "Object interactions",
    ),
    (
        "b:ObjectData.cs",
        "domain.interactions",
        "Object interactions",
    ),
    (
        "n:Terraria.GameContent.NetModules",
        "domain.net-modules",
        "Netcode module content",
    ),
    (
        "n:Terraria.Social",
        "domain.social",
        "Platform social integration",
    ),
    ("n:Terraria.Chat", "domain.chat", "Chat & commands"),
    (
        "n:Terraria.GameContent.UI",
        "ui.content",
        "Content-driven UI screens",
    ),
    ("n:Terraria.UI", "ui.shell", "Menu shell"),
    (
        "n:Terraria.Graphics.Renderers",
        "render.graphics",
        "Graphics & render pipeline",
    ),
    (
        "n:Terraria.Graphics",
        "render.graphics",
        "Graphics & render pipeline",
    ),
    (
        "b:Lighting.cs",
        "render.lighting",
        "Lighting & tile lighting",
    ),
    ("n:Terraria.Audio", "render.audio", "Audio engine"),
    (
        "n:Terraria.Cinematics",
        "render.cinematics",
        "Cinematics & camera",
    ),
    ("n:Terraria.GameInput", "infra.input", "Input handling"),
    (
        "n:Terraria.DataStructures",
        "infra.datastructures",
        "Core data structures",
    ),
    (
        "n:Terraria.Localization",
        "infra.localization",
        "Localization",
    ),
    ("b:Lang.cs", "infra.localization", "Localization"),
    (
        "b:AssemblyInfo.cs",
        "infra.build",
        "Assembly metadata & build attributes",
    ),
    ("n:Terraria.ID", "infra.ids", "Numeric ID registries"),
    ("n:Terraria.Enums", "infra.enums", "Enumerations"),
    (
        "n:Terraria.Modules",
        "infra.modules",
        "Module reflection table",
    ),
    ("n:Terraria.Physics", "infra.math", "Math & utilities"),
    ("n:Terraria.Utilities", "infra.math", "Math & utilities"),
    (
        "n:Terraria.Initializers",
        "infra.setup",
        "One-time system setup",
    ),
    ("n:Terraria.Testing", "test.harness", "In-game test harness"),
    // catch-alls last: the flat GameContent namespace, then bare root-namespace
    (
        "n:Terraria.GameContent",
        "domain.game-content",
        "Assorted game content",
    ),
    ("x:Terraria", "core.runtime", "Core runtime primitives"),
];

/// Namespace as declared in the source; falls back to the directory chain.
pub fn namespace_of(rel: &str, src: Option<&[char]>) -> String {
    if let Some(s) = src {
        if let Some(n) = find_namespace(s) {
            return n;
        }
    }
    let dirs: Vec<&str> = rel
        .rsplit_once('/')
        .map_or(vec![], |(d, _)| d.split('/').collect());
    if dirs.is_empty() {
        return String::new();
    }
    dirs.iter()
        .map(|d| {
            let mut c = d.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

pub fn module_of(rel: &str, src: Option<&[char]>) -> (&'static str, &'static str) {
    let ns = namespace_of(rel, src);
    let base = rel.rsplit('/').next().unwrap_or(rel);
    for (key, module_id, system) in MODULE_RULES.iter() {
        let k = key.as_bytes();
        match k[0] {
            b'b' => {
                if base == &key[2..] {
                    return (module_id, system);
                }
            }
            b'x' => {
                if ns == key[2..] {
                    return (module_id, system);
                }
            }
            _ => {
                if ns == key[2..] || ns.starts_with(&format!("{}.", &key[2..])) {
                    return (module_id, system);
                }
            }
        }
    }
    ("unmapped", "UNMAPPED")
}

pub fn system_of(module_id: &str) -> Option<&'static str> {
    MODULE_RULES
        .iter()
        .find(|(_, mid, _)| *mid == module_id)
        .map(|(_, _, s)| *s)
}

pub fn is_known_module(module_id: &str) -> bool {
    MODULE_RULES.iter().any(|(_, mid, _)| *mid == module_id)
}

#[allow(dead_code)]
pub fn _unused(_p: &Path) {}
