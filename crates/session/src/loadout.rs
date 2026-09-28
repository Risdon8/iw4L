use asset_game::WeaponRegistry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoritativeClassProjection {
    pub def: sim::ClassDef,
    pub lock_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassRow {
    pub weapons: [String; 4],
    pub attachments: [Vec<String>; 2],
    pub perks: [String; 3],
    pub deathstreak: String,
}

pub fn resolve_class_weapon(
    weapons: &WeaponRegistry,
    name: &str,
    attachments: &[String],
    rules: asset_game::LoadoutRules,
) -> Result<u32, String> {
    if name.is_empty() {
        return if attachments.is_empty() {
            Ok(0)
        } else {
            Err("weapon.unknown_family".to_owned())
        };
    }
    let resolve = |selection: asset_game::WeaponSelection| {
        weapons
            .resolve_configuration(&selection, rules)
            .map(|resolved| resolved.id)
            .map_err(|refusal| format!("{name}:{}", refusal.code()))
    };
    if let Some(family) = asset_game::FamilyKey::parse(name)
        && weapons.weapon_families().family(&family).is_some()
    {
        return resolve(asset_game::WeaponSelection::with(family, attachments));
    }
    let id = match weapons.resolve_index(name) {
        Ok(Some(id)) => id,
        Ok(None) | Err(_) => return Err(format!("{name}:catalog.unknown")),
    };
    match weapons.describe_configuration(id) {
        Some(described) if described.family.is_some() => {
            let mut selection = described.clone();
            for extra in attachments {
                if !selection.attachments.contains(extra) {
                    selection.attachments.push(extra.clone());
                }
            }
            resolve(selection)
        }
        _ if attachments.is_empty() => weapons
            .configuration_admission(id)
            .map(|()| id)
            .map_err(|refusal| format!("{name}:{}", refusal.code())),
        _ => Err(format!("{name}:weapon.unknown_family")),
    }
}

pub fn authoritative_class_lock_reason(
    names: [&str; 4],
    ids: [u32; 4],
    combat: &[weapon_iw4::WeaponCombatFacts],
    equipment: &[sim::EquipmentRuntimeFacts],
) -> Option<String> {
    let mut reason = None;
    for (name, id) in names[..2].iter().zip(&ids[..2]) {
        if *id == 0 {
            continue;
        }
        if !combat
            .get(*id as usize)
            .copied()
            .is_some_and(weapon_iw4::WeaponCombatFacts::is_usable)
        {
            append_lock_reason(&mut reason, format!("{name}:validated.missing_profile"));
        }
    }
    for (name, id) in names[2..].iter().zip(&ids[2..]) {
        if *id == 0 {
            continue;
        }
        if !equipment
            .get(*id as usize)
            .copied()
            .is_some_and(sim::EquipmentRuntimeFacts::is_offhand)
        {
            append_lock_reason(&mut reason, format!("{name}:offhand.missing_runtime_facts"));
        }
    }
    reason
}

fn append_lock_reason(reason: &mut Option<String>, item: String) {
    match reason {
        Some(reason) => {
            reason.push_str("; ");
            reason.push_str(&item);
        }
        None => *reason = Some(item),
    }
}

pub fn perk_catalog_id(reference: &str) -> Option<u32> {
    sim::match_state::CLASS_CATALOG_PERKS
        .iter()
        .position(|perk| perk.eq_ignore_ascii_case(reference))
        .map(|index| index as u32 + 1)
}

pub fn project_class(
    class_id: u32,
    row: &ClassRow,
    weapons: &WeaponRegistry,
    combat: &[weapon_iw4::WeaponCombatFacts],
    equipment: &[sim::EquipmentRuntimeFacts],
) -> AuthoritativeClassProjection {
    let rules = asset_game::LoadoutRules::for_class(&row.perks[0]);
    let mut lock_reason = None;
    let no_attachments = Vec::new();
    let mut ids = [0u32; 4];
    for (slot, id) in ids.iter_mut().enumerate() {
        let attachments = row.attachments.get(slot).unwrap_or(&no_attachments);
        match resolve_class_weapon(weapons, &row.weapons[slot], attachments, rules) {
            Ok(resolved) => *id = resolved,
            Err(reason) => append_lock_reason(&mut lock_reason, reason),
        }
    }
    let names = [
        row.weapons[0].as_str(),
        row.weapons[1].as_str(),
        row.weapons[2].as_str(),
        row.weapons[3].as_str(),
    ];
    let mut def = sim::ClassDef::primary_secondary(sim::ClassId(class_id), 1, ids[0], ids[1]);
    def.lethal = ids[2];
    def.tactical = ids[3];
    if let Some(reason) = authoritative_class_lock_reason(names, ids, combat, equipment) {
        append_lock_reason(&mut lock_reason, reason);
    }
    let perks = [
        row.perks[0].as_str(),
        row.perks[1].as_str(),
        row.perks[2].as_str(),
    ];
    let deathstreak = row.deathstreak.as_str();
    for (slot, perk) in def.perks.iter_mut().zip(perks) {
        if perk.is_empty() || perk == "specialty_null" {
            continue;
        }
        match perk_catalog_id(perk) {
            Some(id) => *slot = id,
            None => append_lock_reason(
                &mut lock_reason,
                format!("{perk}:perk.unknown_catalog_entry"),
            ),
        }
    }
    def.deathstreak = if deathstreak.is_empty() || deathstreak == "specialty_null" {
        String::new()
    } else {
        deathstreak.to_owned()
    };
    def.locked = lock_reason.is_some();
    AuthoritativeClassProjection { def, lock_reason }
}
