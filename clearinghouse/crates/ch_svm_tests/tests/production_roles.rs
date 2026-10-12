//! A7: on the **production build** (`pyth,venue-cpi`, loaded from
//! `target/deploy-prod/ch_core.so`), `initialize`, `set_roles` and
//! `accept_admin` reject any configuration where admin, risk_authority,
//! venue_manager and guardian are not four distinct keys.

use anchor_lang::prelude::Pubkey;
use ch_core::{accounts as acc, instruction as ins, ClearinghouseError as E, ConfigParams};
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

/// Production bounds: timelock >= 216,000 slots, hedge break enabled.
fn prod_params() -> ConfigParams {
    ConfigParams {
        timelock_slots: 216_000,
        ..default_params()
    }
}

struct Keys {
    admin: Kp,
    risk: Pubkey,
    vm: Pubkey,
    guard: Pubkey,
}

fn env_with_keys() -> (Env, Keys) {
    let mut env = Env::new_prod();
    let admin = env.funded(10);
    let k = Keys {
        admin,
        risk: kp_pubkey(&env.funded(1)),
        vm: kp_pubkey(&env.funded(1)),
        guard: kp_pubkey(&env.funded(1)),
    };
    (env, k)
}

#[test]
fn prod_initialize_rejects_shared_role_keys() {
    // Each of the six pairs collapsed in turn.
    for (i, j) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
        let (mut env, k) = env_with_keys();
        let ak = kp_pubkey(&k.admin);
        let mut r = [ak, k.risk, k.vm, k.guard];
        r[j] = r[i];
        let tx = env.send(
            &[initialize_roles_ix(&r[0], prod_params(), r[1], r[2], r[3])],
            &[&k.admin],
        );
        assert_ch_err(&tx, E::RolesNotDistinct);
    }
    // The localnet shortcut (every role = admin) is rejected too.
    let (mut env, k) = env_with_keys();
    let ak = kp_pubkey(&k.admin);
    assert_ch_err(
        &env.send(&[initialize_ix(&ak, prod_params())], &[&k.admin]),
        E::RolesNotDistinct,
    );
    // Four distinct keys: accepted, and the roles are stored.
    let (mut env, k) = env_with_keys();
    let ak = kp_pubkey(&k.admin);
    assert_ok(&env.send(
        &[initialize_roles_ix(
            &ak,
            prod_params(),
            k.risk,
            k.vm,
            k.guard,
        )],
        &[&k.admin],
    ));
    let c: ch_core::GlobalConfig = env.account(&config_pda()).unwrap();
    assert_eq!(
        (c.admin, c.risk_authority, c.venue_manager, c.guardian),
        (ak, k.risk, k.vm, k.guard)
    );
}

fn initialized() -> (Env, Keys) {
    let (mut env, k) = env_with_keys();
    let ak = kp_pubkey(&k.admin);
    assert_ok(&env.send(
        &[initialize_roles_ix(
            &ak,
            prod_params(),
            k.risk,
            k.vm,
            k.guard,
        )],
        &[&k.admin],
    ));
    (env, k)
}

#[test]
fn prod_set_roles_rejects_shared_keys() {
    let (mut env, k) = initialized();
    let ak = kp_pubkey(&k.admin);
    let cases = [
        (ak, k.vm, k.guard),       // risk = admin
        (k.risk, k.risk, k.guard), // risk = venue manager
        (k.risk, k.vm, k.vm),      // venue manager = guardian
        (k.risk, k.vm, ak),        // guardian = admin
    ];
    for (r, v, g) in cases {
        assert_ch_err(
            &env.send(&[set_roles_ix(&ak, r, v, g)], &[&k.admin]),
            E::RolesNotDistinct,
        );
    }
    let new_guard = kp_pubkey(&env.funded(1));
    assert_ok(&env.send(&[set_roles_ix(&ak, k.risk, k.vm, new_guard)], &[&k.admin]));
}

#[test]
fn prod_accept_admin_rejects_role_key() {
    let (mut env, k) = initialized();
    let ak = kp_pubkey(&k.admin);
    let propose = |new_admin: Pubkey| {
        ix(
            acc::AdminOnly {
                config: config_pda(),
                admin: ak,
            },
            ins::ProposeAdmin { new_admin },
        )
    };
    let accept = |who: Pubkey| {
        ix(
            acc::AcceptAdmin {
                config: config_pda(),
                proposed_admin: who,
            },
            ins::AcceptAdmin {},
        )
    };
    // The guardian's key cannot become admin.
    let guard_kp = env.funded(1);
    let gk = kp_pubkey(&guard_kp);
    assert_ok(&env.send(&[set_roles_ix(&ak, k.risk, k.vm, gk)], &[&k.admin]));
    assert_ok(&env.send(&[propose(gk)], &[&k.admin]));
    assert_ch_err(&env.send(&[accept(gk)], &[&guard_kp]), E::RolesNotDistinct);
    // A fresh key can.
    let fresh = env.funded(1);
    let fk = kp_pubkey(&fresh);
    assert_ok(&env.send(&[propose(fk)], &[&k.admin]));
    assert_ok(&env.send(&[accept(fk)], &[&fresh]));
    let c: ch_core::GlobalConfig = env.account(&config_pda()).unwrap();
    assert_eq!(c.admin, fk);
}
