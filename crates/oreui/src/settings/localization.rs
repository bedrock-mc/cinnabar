//! Built-in settings text shared by rows, sidebar categories and choice pickers.

use launcher::menu::settings_options::custom_key_label;
use ui_engine::engine::screen::Translate;

/// Resolves settings text with the French additions, active catalog and English fallback.
pub(super) fn word(key: &str, language: Option<&str>, translate: Translate<'_>) -> String {
    if key.is_empty() {
        return String::new();
    }
    if let Some(text) = locale_text(key, language) {
        return text.to_owned();
    }
    translate(key).map_or_else(|| fallback(key).to_owned(), |text| text.to_string())
}

/// Returns built-in text only when the selected locale belongs to its language.
pub(super) fn locale_text(key: &str, language: Option<&str>) -> Option<&'static str> {
    language
        .filter(|language| language.starts_with("fr"))
        .and_then(|_| french_text(key))
}

/// Returns built-in French settings text for supported localization keys.
pub(super) fn french_text(key: &str) -> Option<&'static str> {
    Some(match key {
        "options.on" | "On" | "gui.yes" | "True" | "true" => "Oui",
        "options.off" | "Off" | "gui.no" | "False" | "false" => "Non",
        "Automatic" => "Automatique",
        "Low" => "Faible",
        "Medium" => "Moyen",
        "High" => "Élevé",
        "Reset" => "Réinitialiser",
        "Open" => "Ouvrir",
        "View" => "Afficher",
        "Clear data" => "Effacer les données",
        "Sign In" => "Se connecter",
        "Sign Out" => "Se déconnecter",
        "Unassigned" => "Non attribué",

        "options.cinnabar.advanced" => "Avancé",
        "options.renderClouds" => "Afficher les nuages",
        "options.renderClouds.description" => "Affiche les nuages dans le ciel.",
        "options.showExactServerPing" => "Afficher le ping exact du serveur",
        "options.showExactServerPing.description" => {
            "Affiche la latence du serveur en millisecondes au lieu de Faible, Moyen ou Élevé."
        }
        "options.oreuiDarkMode" => "Mode sombre",
        "options.oreuiDarkMode.description" => {
            "Utiliser des surfaces sombres pour les menus, paramètres et boîtes de dialogue."
        }
        "options.framerateLimit.description" => {
            "Choisissez une limite d'IPS ou Illimité. La VSync ou le taux de rafraîchissement variable peuvent toujours limiter la fréquence."
        }
        "options.framerateLimit.max" => "Illimité",
        "options.fullscreen" => "Plein écran",
        "options.fullscreen.description" => "Activer ou désactiver l'affichage plein écran",
        "options.guiScale.optionName.name" => "Taille de l'interface",
        "options.guiScale.optionName.description" => {
            "Ajustez la taille du HUD et des éléments de l'interface pour faciliter leur utilisation."
        }
        "options.guiScale.disabled" => {
            "La mise à l'échelle de l'interface n'est pas prise en charge sur cet écran."
        }
        "options.guiAccessibilityScaling.name" => "Nouvelle interface très grande",
        "options.guiAccessibilityScaling.description" => {
            "Augmenter la taille de l'interface pour plus de clarté et une lecture facile"
        }

        "options.cinnabar.worldsFolder" => "Dossier des mondes",
        "options.cinnabar.worldsFolder.description" => "Emplacement où vos mondes sont enregistrés",
        "options.cinnabar.resourcePacksFolder" => "Dossier des packs de ressources",
        "options.cinnabar.resourcePacksFolder.description" => {
            "Emplacement où vos packs de ressources importés sont stockés"
        }
        "options.cinnabar.shaderPacksFolder" => "Dossier des shaders",
        "options.cinnabar.shaderPacksFolder.description" => {
            "Ajoutez des shaders Java ici ou glissez-déposez-les sur cette fenêtre"
        }
        "options.cinnabar.openFolder.buttonLabel" => "Ouvrir le dossier",
        "options.cinnabar.shaderOptions" => "Paramètres du shader",
        "options.cinnabar.shaderOptions.description" => {
            "Options propres au pack sélectionné. Les appliquer recharge le pack."
        }
        "options.cinnabar.shaderOptions.buttonLabel" => "Options du shader…",

        "options.f5SelfVisuals" | "options.f5SelfVisuals.name" => "Visuel F5 immersif",
        "options.f5SelfVisuals.description" => {
            "Affiche le joueur complet en vue F5 (armure, cosmétiques, cape et pseudo tels que les autres joueurs vous voient)."
        }
        "options.estheticTab" | "options.estheticTab.name" => "Tab esthétique (Style Lunar)",
        "options.estheticTab.description" => {
            "Affiche le menu Tab avec l'en-tête du serveur, fond moderne translucide et indicateurs de ping esthétiques."
        }

        "menu.settings" | "hbui.Settings.title" => "Paramètres",
        "menu.video.tab.title" | "options.video" => "Vidéo",
        "menu.video.tab.description" => "Ajuster les graphismes et la qualité visuelle",
        "menu.video.group.general" => "Général",
        "menu.video.group.general.description" => "Paramètres vidéo courants",
        "menu.video.group.performance" => "Graphismes, performances et disposition",
        "menu.video.group.performance.description" => {
            "Contrôlez l'apparence et la fluidité de Minecraft"
        }
        "menu.video.group.customization" => "Personnalisation de l'affichage",
        "menu.video.group.customization.description" => {
            "Construisez, streamez ou prenez des captures sans distractions"
        }
        "menu.video.group.accessibility" => "Accessibilité : Vidéo",
        "menu.video.group.accessibility.description" => {
            "Commandes pour rendre Minecraft plus agréable à jouer"
        }
        "menu.video.mode" => "Mode graphique",
        "options.graphicsModeOptions.fancy" => "Graphismes détaillés",
        "options.graphicsModeOptions.simple" => "Graphismes simples",

        "menu.accessibility.tab.title" | "options.accessibility" => "Accessibilité",
        "menu.accessibility.tab.description" => "Minecraft est pour tout le monde, y compris vous",
        "menu.accessibility.tab.tts.title" => "Synthèse vocale",
        "menu.accessibility.tab.tts.description" => {
            "Ajustez la lecture vocale des textes à l'écran"
        }
        "menu.accessibility.tab.gameplay.title" => "Jouabilité",
        "menu.accessibility.tab.gameplay.description" => {
            "Modifier les options d'accessibilité pour les mouvements de caméra et le visuel"
        }
        "menu.accessibility.tab.ui.title" => "Interface utilisateur",
        "menu.accessibility.tab.ui.description" => {
            "Adapter l'interface du jeu à votre style de jeu"
        }

        "menu.keyboardAndMouse.tab.title" | "options.keyboardAndMouse" => "Clavier et souris",
        "menu.keyboardAndMouse.tab.description" => {
            "Ajuster les options d'entrée et l'attribution des touches"
        }
        "menu.keyboardAndMouse.tab.mappings.title" => "Attribution des touches",
        "menu.keyboardAndMouse.tab.mappings.description" => {
            "Réattribuez toutes les touches clavier & souris pour jouer à votre façon"
        }
        "menu.keyboardAndMouse.tab.macros.title" => "Macros",
        "menu.keyboardAndMouse.tab.macros.description" => {
            "Exécutez une commande avec la touche assignée et le modificateur de macro sélectionné."
        }
        "options.commandMacro.placeholder" => "Commande à exécuter",

        "menu.controller.tab.title" | "options.controller" => "Manette",
        "menu.controller.tab.description" => {
            "Options d'entrée, sensibilité et configuration des boutons de la manette"
        }
        "menu.controller.tab.mappings.title" => "Configuration des touches",
        "menu.controller.tab.mappings.description" => {
            "Réattribuez les touches de la manette pour jouer à votre façon"
        }

        "menu.general.tab.title" | "options.general" => "Général",
        "menu.general.tab.description" => "Divers paramètres, du réseau aux crédits du jeu",
        "menu.general.tab.pause.title" => "Pause",
        "menu.general.tab.pause.description" => "Faire une pause lors d'une partie en solo",
        "menu.general.tab.network.title" => "Paramètres réseau",
        "menu.general.tab.network.description" => {
            "Ajuster l'expérience en ligne au niveau technique et social"
        }
        "menu.general.tab.miscellaneous.title" => "Divers",
        "menu.general.tab.miscellaneous.description" => "Réglages supplémentaires",
        "menu.general.tab.tutorial.title" => "Tutoriel",
        "menu.general.tab.tutorial.description" => "Aide pour apprendre à jouer",

        "menu.audio.tab.title" | "options.audio" => "Audio",
        "menu.audio.tab.description" => "Régler le volume pour la musique et les effets sonores",

        "menu.account.tab.title" | "options.account" => "Compte",
        "menu.account.tab.description" => "Accéder aux informations de votre compte",
        "menu.account.gamertag.title" => "Gamertag",
        "menu.account.changeGamertag.title" => "Changer de Gamertag",
        "menu.account.manageAccount.title" => "Gérer le compte",
        "menu.account.privacyAndSafety.title" => "Confidentialité et sécurité en ligne",
        "menu.account.realmMembershipInvites.title" => "Gérer les invitations Realms",
        "menu.account.signOutOfMicrosoft.title" => "Se déconnecter de votre compte Microsoft",
        "menu.account.signIn.title" => "Se connecter gratuitement",
        "menu.account.changeGamertag.buttonLabel"
        | "menu.account.manageAccount.buttonLabel"
        | "menu.account.privacyAndSafety.buttonLabel" => "Ouvrir",
        "menu.account.realmMembershipInvites.buttonLabel" => "Gérer",
        "menu.account.signOutOfMicrosoft.buttonLabel" => "Déconnexion",
        "menu.account.signIn.buttonLabel" => "Connexion",
        "menu.account.clearSignInData.buttonLabel" => "Effacer",

        "menu.language.tab.title" | "options.language" => "Langue",
        "menu.language.tab.description" => "Sélectionnez votre langue préférée pour Minecraft",

        "menu.storage.tab.title" | "options.storage" => "Stockage",
        "menu.storage.tab.description" => {
            "Gérer les mondes, packs de ressources, packs de comportement et données en cache"
        }
        "hbui.Settings.storage.worlds.title" => "Mondes",
        "hbui.Settings.storage.worldTemplates.title" => "Modèles de monde",
        "hbui.Settings.storage.resourcePacks.title" => "Packs de ressources",
        "hbui.Settings.storage.behaviorPacks.title" => "Packs de comportement",
        "storageManager.contentType.skinPacks" => "Packs de skins",
        "hbui.Settings.storage.cachedData.title" => "Données en cache",
        "hbui.Settings.storage.emptyList.title" => "Aucun %1$s trouvé",
        "hbui.Settings.storage.listHeader.delete" => "Supprimer",

        "options.globalResources" | "menu.globalpacks" | "options.resourcepacks" => {
            "Ressources globales"
        }
        "resourcePack.selected.title.packs" => "Actifs",
        "resourcePack.available.title.packs" => "Mes packs",
        "resourcePack.message.noneFound.packs" => "Aucun pack disponible sur cet appareil.",
        "resourcePack.selected.remove" => "Désactiver",
        "resourcePack.available.add" => "Activer",

        "options.helpCenter" => "Centre d'aide",
        "options.helpCenter.description" => {
            "Obtenez des réponses à toutes vos questions d'assistance"
        }
        "options.attribution" => "Attribution",
        "options.attribution.description" => "Partenaires technologiques et systèmes",
        "options.licensed_content" => "Contenu sous licence",
        "options.licensed_content.description" => {
            "Tout le contenu sous licence disponible dans Minecraft"
        }
        "options.font_license" => "Licence de police",
        "options.font_license.description" => "Licence pour les polices tierces",
        "options.helpCenter.buttonLabel"
        | "options.attribution.buttonLabel"
        | "options.licensed_content.buttonLabel" => "Ouvrir",
        "options.font_license.buttonLabel" => "Afficher",

        "options.keyboardAndMouse.resetMappings" => "Réinitialiser les touches par défaut",
        "options.keyboardAndMouse.resetMappings.description" => {
            "Restaurer toutes les touches clavier & souris à leurs valeurs d'origine"
        }
        "options.controller.resetMappings" => "Réinitialiser les touches de la manette",
        "options.controller.resetMappings.description" => {
            "Restaurer toutes les touches de la manette à leurs valeurs d'origine"
        }
        "options.accessibility.resetSettings"
        | "options.video.resetSettings"
        | "options.sound.resetSettings" => "Rétablir par défaut",
        "options.accessibility.resetSettings.description" => {
            "Rétablir tous les paramètres d'accessibilité à leurs valeurs d'origine"
        }
        "options.video.resetSettings.description" => {
            "Rétablir tous les paramètres vidéo à leurs valeurs d'origine"
        }
        "options.sound.resetSettings.description" => {
            "Rétablir toutes les options sonores à leurs valeurs d'origine"
        }
        "options.keyboardAndMouse.resetMappings.buttonLabel"
        | "options.controller.resetMappings.buttonLabel"
        | "options.accessibility.resetSettings.buttonLabel"
        | "options.video.resetSettings.buttonLabel"
        | "options.sound.resetSettings.buttonLabel"
        | "options.key.reset.buttonLabel" => "Réinitialiser",

        "options.dev_clearAllCache" => "Effacer tout le cache",
        "options.dev_clearDownloadeCache.name" => "Effacer le cache de téléchargement",
        "options.dev_deleteLocalScreenshots" => "Supprimer les captures d'écran locales",

        "menu.touch.tab.title" | "options.touch" => "Tactile",
        "menu.touch.tab.description" => {
            "Options d'entrée, sensibilité et configuration des boutons tactiles"
        }
        "menu.party.tab.title" | "options.party" => "Groupe",
        "options.partyInviteReceivedFilter" => "Recevoir des invitations de",
        "options.partyPrivacy" => "Confidentialité par défaut",
        "options.partyInviteSendPrivileges" => "Qui peut inviter dans votre groupe",

        _ => return None,
    })
}

/// Returns built-in English text when the active catalog has no settings entry.
pub(super) fn fallback(key: &str) -> &str {
    match key {
        "options.renderClouds" => "Render Clouds",
        "options.cinnabar.advanced" => "Advanced",
        "options.framerateLimit.description" => {
            "Menus stay within display refresh. VSync and Variable Refresh Rate may lower the rate."
        }
        "options.showExactServerPing" => "Show exact server ping",
        "options.oreuiDarkMode" => "Dark Mode",
        "options.oreuiDarkMode.description" => "Use dark surfaces for menus, settings and dialogs.",
        "options.showExactServerPing.description" => {
            "Show server latency in milliseconds instead of Low, Medium or High ping."
        }
        "options.renderClouds.description" => "Show clouds in the sky.",
        "hbui.Settings.title" => "Settings",
        "hbui.Settings.category.controls" | "options.controls" | "options.group.input" => {
            "Controls"
        }
        "hbui.Settings.category.social" | "options.social" => "Social",
        "options.chat.title" | "chat.settings.title" => "Chat",
        "options.chat.title.description" => "Chat appearance, opacity, size and colors",
        "chat.settings.reset" => "Reset Chat Settings",
        "chat.settings.reset.description" => "Restore all chat options to default values",
        "chat.settings.reset.buttonLabel" => "Reset",
        "chat.settings.muteAll" => "Mute All Chat",
        "chat.settings.muteAll.description" => "Hide all incoming chat messages",
        "chat.settings.muteEmotes" => "Mute Emotes",
        "chat.settings.muteEmotes.description" => {
            "Saved preference; emote chat filtering is not available yet"
        }
        "chat.settings.tts" => "Text to Speech for Chat",
        "chat.settings.tts.description" => "Saved preference; chat narration is not available yet",
        "chat.settings.fontSize" => "Font Size",
        "chat.settings.fontSize.description" => "Adjust the scale of chat text",
        "chat.settings.lineSpacing" => "Line Spacing",
        "chat.settings.lineSpacing.description" => "Set the spacing between lines in chat",
        "chat.settings.chatColor" => "Chat Color",
        "chat.settings.chatColor.description" => "Default color for chat text",
        "chat.settings.mentionsColor" => "Mentions Color",
        "chat.settings.mentionsColor.description" => {
            "Saved preference; mention highlighting is not available yet"
        }
        "options.chatBackgroundOpacity" => "Chat Background Opacity",
        "options.chatBackgroundOpacity.description" => {
            "Adjust the opacity of the chat background box"
        }
        "options.notificationDuration.Chat" => "Chat Notification Duration",
        "options.notificationDuration.Chat.description" => {
            "How long chat notifications remain visible on the HUD"
        }
        "Discord" | "discord.settings.title" => "Discord",
        "discord.settings.description" => {
            "Control what Cinnabar shares with the Discord desktop app"
        }
        "discord.settings.enabled" => "Discord Rich Presence",
        "discord.settings.enabled.description" => {
            "Publish your current Cinnabar activity to the Discord desktop app"
        }
        "discord.settings.details.title" => "Activity details",
        "discord.settings.details.description" => {
            "Choose which optional information the Discord activity card can show"
        }
        "discord.settings.serverDetails" => "Show server and world details",
        "discord.settings.serverDetails.description" => {
            "Display the server address or local world name in the activity card"
        }
        "discord.settings.partySize" => "Show party size",
        "discord.settings.partySize.description" => {
            "Display the current and maximum player count when available"
        }
        "discord.settings.allowJoins" => "Allow Discord joins",
        "discord.settings.allowJoins.description" => {
            "Let Discord friends join through the activity card and send join requests"
        }
        "discord.settings.timestamp" => "Show elapsed time",
        "discord.settings.timestamp.description" => {
            "Show how long you have been in the current activity"
        }
        "discord.settings.reset" => "Reset Discord Settings",
        "discord.settings.reset.description" => "Restore all Discord options to default values",
        "discord.settings.reset.buttonLabel" => "Reset",
        "color.white" => "White",
        "color.green" => "Green",
        "color.aqua" => "Aqua",
        "color.red" => "Red",
        "color.light_purple" => "Light Purple",
        "color.yellow" => "Yellow",
        "color.gold" => "Gold",
        "color.gray" => "Gray",
        "color.dark_gray" => "Dark Gray",
        "color.blue" => "Blue",
        "color.dark_green" => "Dark Green",
        "color.dark_aqua" => "Dark Aqua",
        "color.dark_purple" => "Dark Purple",
        "color.dark_red" => "Dark Red",
        "color.dark_blue" => "Dark Blue",
        "color.black" => "Black",
        "options.f5SelfVisuals" | "options.f5SelfVisuals.name" => "Third-Person Self Visuals",
        "options.f5SelfVisuals.description" => {
            "Display nametag, score, badges, and status overhead on your own player in third-person view."
        }
        "Chat Position" => "Chat Position",
        "Chat Position.description" => "Position the chat HUD at the bottom or top of the screen",
        "hbui.Settings.category.general" => "General",
        "menu.accessibility.tab.title" | "options.accessibility" => "Accessibility",
        "menu.accessibility.tab.description" => "Minecraft is for everyone, including you",
        "menu.accessibility.tab.gameplay.title" => "Gameplay",
        "menu.accessibility.tab.gameplay.description" => {
            "Change accessibility options for in-game visuals and camera movements"
        }
        "menu.accessibility.tab.ui.title" => "User Interface",
        "menu.accessibility.tab.ui.description" => "Set the game’s UI to fit your playing style",
        "menu.keyboardAndMouse.tab.title" | "options.keyboardAndMouse" => "Keyboard & Mouse",
        "menu.keyboardAndMouse.tab.description" => "Adjust input and mapping options",
        "menu.keyboardAndMouse.tab.mappings.title" => "Keyboard & Mouse Mappings",
        "menu.keyboardAndMouse.tab.mappings.description" => {
            "Remap any and all keyboard & mouse buttons to play your way"
        }
        "menu.keyboardAndMouse.tab.macros.title" => "Macros",
        "menu.keyboardAndMouse.tab.macros.description" => {
            "Run a command with its bound control and the Macro Modifier selected above. Choose None to use the control alone."
        }
        "options.commandMacro.placeholder" => "Command to run",
        "menu.controller.tab.title" | "options.controller" => "Controller",
        "menu.controller.tab.description" => {
            "Input options, sensitivity settings and button mapping for controllers"
        }
        "menu.controller.tab.mappings.title" => "Button mapping",
        "menu.controller.tab.mappings.description" => {
            "Remap any and all controller buttons to play your way"
        }
        "menu.general.tab.title" | "options.general" => "General",
        "menu.general.tab.description" => "A bunch of stuff, from network settings to game credits",
        "menu.general.tab.pause.title" => "Pause",
        "menu.general.tab.pause.description" => "Take a break while playing on your own",
        "menu.general.tab.network.title" => "Network Settings",
        "menu.general.tab.network.description" => {
            "Adjust the online experience on a technical and social level"
        }
        "menu.general.tab.miscellaneous.title" => "Miscellaneous",
        "menu.general.tab.miscellaneous.description" => {
            "We didn’t know where else to put these settings"
        }
        "menu.general.tab.tutorial.title" => "Tutorial",
        "menu.general.tab.tutorial.description" => "Help learning the game",
        "menu.creator.tab.title" | "options.creator" => "Creator",
        "menu.video.tab.title" | "options.video" => "Video",
        "menu.video.tab.description" => "Adjust graphics and visual quality",
        "menu.video.group.general" => "General",
        "menu.video.group.general.description" => "Common video settings",
        "menu.video.group.performance" => "Graphics, performance and layout",
        "menu.video.group.performance.description" => "Control how Minecraft looks and runs",
        "menu.video.group.customization" => "View customization",
        "menu.video.group.customization.description" => {
            "Build, stream, or take screenshots without distractions"
        }
        "menu.video.group.accessibility" => "Accessibility: Video",
        "menu.video.group.accessibility.description" => {
            "Controls to make Minecraft more comfortable to play"
        }
        "menu.video.mode" => "Mode",
        "options.graphicsModeOptions.fancy" => "Fancy Graphics Options",
        "options.graphicsModeOptions.simple" => "Simple Graphics Options",
        "menu.audio.tab.title" | "options.audio" => "Audio",
        "menu.audio.tab.description" => "Adjust volume settings for music and sound effects",
        "menu.account.tab.title" | "options.account" => "Account",
        "menu.account.tab.description" => "Access your account information",
        "menu.account.gamertag.title" => "Gamertag",
        "menu.account.changeGamertag.title" => "Change Gamertag",
        "menu.account.manageAccount.title" => "Manage Account",
        "menu.account.signOutOfMicrosoft.title" => "Sign out of your Microsoft account",
        "menu.account.signIn.title" => "Sign in for free",
        "menu.account.changeGamertag.buttonLabel" | "menu.account.manageAccount.buttonLabel" => {
            "Open"
        }
        "menu.account.signOutOfMicrosoft.buttonLabel" => "Sign Out",
        "menu.account.signIn.buttonLabel" => "Sign In",
        "menu.account.clearSignInData.buttonLabel" => "Clear data",
        "menu.language.tab.title" | "options.language" => "Language",
        "menu.language.tab.description" => "Select your preferred language for Minecraft",
        "menu.storage.tab.title" | "options.storage" => "Storage",
        "menu.storage.tab.description" => {
            "Manage worlds, world templates, resource packs, behavior packs and cached data"
        }
        "hbui.Settings.storage.worlds.title" => "Worlds",
        "hbui.Settings.storage.worldTemplates.title" => "World templates",
        "hbui.Settings.storage.resourcePacks.title" => "Resource packs",
        "hbui.Settings.storage.behaviorPacks.title" => "Behavior packs",
        "storageManager.contentType.skinPacks" => "Skin Packs",
        "hbui.Settings.storage.cachedData.title" => "Cached data",
        "hbui.Settings.storage.emptyList.title" => "No %1$s found",
        "hbui.Settings.storage.listHeader.delete" => "Delete",
        "options.cinnabar.worldsFolder" => "Worlds folder",
        "options.cinnabar.worldsFolder.description" => "Where your worlds are saved",
        "options.cinnabar.resourcePacksFolder" => "Resource packs folder",
        "options.cinnabar.resourcePacksFolder.description" => {
            "Where your imported resource packs are stored"
        }
        "options.cinnabar.shaderPacksFolder" => "Shader packs folder",
        "options.cinnabar.shaderPacksFolder.description" => {
            "Add Java shader packs here, or drop them onto this window"
        }
        "options.cinnabar.openFolder.buttonLabel" => "Open folder",
        "options.cinnabar.shaderOptions" => "Shader pack settings",
        "options.cinnabar.shaderOptions.description" => {
            "The selected pack's own options. Applying them reloads the pack."
        }
        "options.cinnabar.shaderOptions.buttonLabel" => "Shader options…",
        "options.dev_clearAllCache" => "Clear All Cache",
        "options.dev_clearDownloadeCache.name" => "Clear download cache",
        "options.dev_deleteLocalScreenshots" => "Delete Local Screenshots",
        "playscreen.fileSize.GB" => "GB",
        "playscreen.fileSize.MB" => "MB",
        "menu.touch.tab.title" | "options.touch" => "Touch",
        "menu.touch.tab.description" => {
            "Input options, sensitivity settings and button mapping for touch screens"
        }
        "hudScreen.controlCustomization.tooltip.notouch" => {
            "Use a touch device to customize controls"
        }
        "options.globalResources" | "menu.globalpacks" | "options.resourcepacks" => {
            "Global Resources"
        }
        "resourcePack.selected.title.packs" => "Active",
        "resourcePack.available.title.packs" => "My Packs",
        "resourcePack.message.noneFound.packs" => "There are no available packs on this device.",
        "resourcePack.selected.remove" => "Deactivate",
        "resourcePack.available.add" => "Activate",
        "resourcePack.description.bottom.global" => {
            "Resource packs are applied bottom to top. This means any asset that is in two packs will be overridden by the higher pack. Packs in your worlds will apply on top of these global packs. These resources are just for you. No one else will see the resources you set here. Resource Packs in your worlds or worlds you join will apply on top of these global resources."
        }
        "menu.settings" => "Settings",
        "gui.select" => "Select",
        "gui.clear" => "Clear",
        "gui.back" => "Back",
        "options.helpCenter" => "Help Center",
        "options.helpCenter.description" => "Get answers to all of your burning support queries",
        "options.attribution" => "Attribution",
        "options.attribution.description" => "Technology partners and systems",
        "options.licensed_content" => "Licensed Content",
        "options.licensed_content.description" => "All licensed content available in Minecraft",
        "options.font_license" => "Font License",
        "options.font_license.description" => "A license for third-party fonts",
        "options.helpCenter.buttonLabel"
        | "options.attribution.buttonLabel"
        | "options.licensed_content.buttonLabel" => "Open",
        "options.font_license.buttonLabel" => "View",
        "options.guiScale.optionName.name" => "UI scale modifier",
        "options.guiScale.disabled" => {
            "UI scaling is unsupported on this monitor, most likely because of a low resolution."
        }
        "options.renderDistanceFormat" => "%s chunks",
        "options.framerateLimit.max" => "Unlimited",
        "options.guiScale.optionName.description" => {
            "Select the size of the HUD and other UI elements to make them easier to use on different devices"
        }
        "options.guiAccessibilityScaling.name" => "Extra large new UI",
        "options.guiAccessibilityScaling.description" => {
            "Increase the size of the UI for clarity and easy reading"
        }
        "options.keyboardAndMouse.resetMappings" => "Reset to default mapping",
        "options.keyboardAndMouse.resetMappings.description" => {
            "Restore all keyboard & mouse button mappings to their original values"
        }
        "options.controller.resetMappings" => "Reset mapping to default",
        "options.controller.resetMappings.description" => {
            "Restore all controller button mappings to their original values"
        }
        "options.accessibility.resetSettings"
        | "options.video.resetSettings"
        | "options.sound.resetSettings" => "Reset to default",
        "options.accessibility.resetSettings.description" => {
            "Return all accessibility settings and sliders to their original values"
        }
        "options.video.resetSettings.description" => {
            "Restore all video settings to their original values"
        }
        "options.sound.resetSettings.description" => "Set all options to their original values",
        "options.keyboardAndMouse.resetMappings.buttonLabel"
        | "options.controller.resetMappings.buttonLabel"
        | "options.accessibility.resetSettings.buttonLabel"
        | "options.video.resetSettings.buttonLabel"
        | "options.sound.resetSettings.buttonLabel"
        | "options.key.reset.buttonLabel" => "Reset",
        "controllerLayoutScreen.unassigned" => "Unassigned",
        _ => custom_key_label(key).unwrap_or(key),
    }
}
