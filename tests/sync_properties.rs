//! Propriétés de la fusion (tests déterministes : générateur congruentiel, aucune dépendance externe).

use backlog_panda::library::{Library, Status};
use backlog_panda::rating::Rating;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

const STATUSES: [Status; 6] =
    [Status::Backlog, Status::Playing, Status::Played, Status::Completed, Status::Abandoned, Status::Wishlist];

fn random_library(replica: &str, rng: &mut Lcg, ops: usize) -> Library {
    let mut l = Library::new(replica);
    for _ in 0..ops {
        let id = rng.next() % 6;
        match rng.next() % 4 {
            0 | 1 => {
                let rating = (!rng.next().is_multiple_of(3)).then(|| Rating::from_half_stars((rng.next() % 10 + 1) as u8).unwrap());
                l.upsert(id, STATUSES[(rng.next() % 6) as usize], rating);
            }
            2 => {
                let _ = l.remove(id);
            }
            _ => {
                let _ = l.set_rating(id, Some(Rating::from_half_stars((rng.next() % 10 + 1) as u8).unwrap()));
            }
        }
    }
    l
}

#[test]
fn la_fusion_est_commutative_associative_idempotente() {
    let mut rng = Lcg(42);
    for round in 0..200 {
        let a = random_library("a", &mut rng, 12);
        let b = random_library("b", &mut rng, 12);
        let c = random_library("c", &mut rng, 12);

        let (mut ab, mut ba) = (a.clone(), b.clone());
        ab.merge(&b);
        ba.merge(&a);
        assert_eq!(ab.entries, ba.entries, "commutativité, tour {round}");

        let (mut ab_c, mut bc) = (ab.clone(), b.clone());
        ab_c.merge(&c);
        bc.merge(&c);
        let mut a_bc = a.clone();
        a_bc.merge(&bc);
        assert_eq!(ab_c.entries, a_bc.entries, "associativité, tour {round}");

        let mut twice = ab.clone();
        let report = twice.merge(&ab);
        assert_eq!(twice.entries, ab.entries, "idempotence, tour {round}");
        assert_eq!(report.adopted, 0);
    }
}

#[test]
fn une_suppression_recente_l_emporte_et_se_propage() {
    let mut a = Library::new("a");
    let mut b = Library::new("b");
    a.upsert(1, Status::Playing, None);
    b.merge(&a);
    b.remove(1).unwrap();
    a.merge(&b);
    assert!(a.get(1).is_none());
    // Une réactivation ultérieure (tampon plus grand) ressuscite l'entrée.
    a.upsert(1, Status::Wishlist, None);
    b.merge(&a);
    assert_eq!(b.get(1).unwrap().status, Status::Wishlist);
}

#[test]
fn l_horloge_ne_recule_jamais_apres_fusion() {
    let mut a = Library::new("a");
    let mut b = Library::new("b");
    for i in 0..5 {
        b.upsert(i, Status::Backlog, None);
    }
    a.merge(&b);
    assert_eq!(a.clock, 5);
    a.upsert(9, Status::Playing, None);
    assert!(a.entries[&9].stamp > b.entries[&4].stamp);
}
