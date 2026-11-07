use cfdcraft_runs::{Item, PlanError, Stage, StageState, Status, pending, plan_update};
use serde_json::json;

fn mesh(doc: &str) -> Item {
    Item::new(doc, Stage::Mesh)
}

fn result(doc: &str) -> Item {
    Item::new(doc, Stage::Result)
}

fn stale() -> StageState {
    StageState::Stale {
        since: "run-1".to_owned(),
        changes: Vec::new(),
    }
}

fn fresh() -> StageState {
    StageState::Fresh {
        run: "run-1".to_owned(),
    }
}

#[test]
fn item_text_forms_round_trip() {
    assert_eq!(mesh("doc").to_string(), "doc:mesh");
    assert_eq!(result("doc").to_string(), "doc:result");
    assert_eq!("doc:mesh".parse::<Item>().unwrap(), mesh("doc"));
    assert_eq!("doc:result".parse::<Item>().unwrap(), result("doc"));
    assert_eq!("a:b:mesh".parse::<Item>().unwrap(), mesh("a:b"));
    assert_eq!(
        serde_json::to_value(mesh("doc")).unwrap(),
        json!("doc:mesh")
    );
    let back: Item = serde_json::from_value(json!("doc:result")).unwrap();
    assert_eq!(back, result("doc"));
    for bad in ["a", "a:other", ":mesh", ""] {
        assert_eq!(
            bad.parse::<Item>(),
            Err(PlanError::BadItem(bad.to_owned())),
            "{bad:?}"
        );
    }
    assert!(serde_json::from_value::<Item>(json!("a:other")).is_err());
}

#[test]
fn pending_lists_the_stages_that_are_not_fresh_mesh_first() {
    let both_stale = Status {
        mesh: stale(),
        result: stale(),
    };
    assert_eq!(pending("doc", &both_stale), [mesh("doc"), result("doc")]);
    let result_stale = Status {
        mesh: fresh(),
        result: stale(),
    };
    assert_eq!(pending("doc", &result_stale), [result("doc")]);
    let all_fresh = Status {
        mesh: fresh(),
        result: fresh(),
    };
    assert!(pending("doc", &all_fresh).is_empty());
    let never = Status {
        mesh: StageState::NeverMade,
        result: StageState::NeverMade,
    };
    assert_eq!(pending("doc", &never), [mesh("doc"), result("doc")]);
}

#[test]
fn by_default_everything_pending_is_updated_in_dependency_order() {
    let waiting = [result("b"), mesh("b"), mesh("a"), result("a")];
    assert_eq!(
        plan_update(&waiting, None, None).unwrap(),
        [mesh("b"), result("b"), mesh("a"), result("a")]
    );
}

#[test]
fn one_item_can_be_chosen() {
    let waiting = [mesh("a"), result("a")];
    let chosen = [mesh("a")];
    assert_eq!(
        plan_update(&waiting, Some(&chosen), None).unwrap(),
        [mesh("a")]
    );
    let only_result = [result("a")];
    assert_eq!(
        plan_update(&only_result, Some(&only_result), None).unwrap(),
        [result("a")]
    );
}

#[test]
fn an_item_whose_upstream_is_pending_needs_it_chosen_too() {
    let waiting = [mesh("a"), result("a")];
    let chosen = [result("a")];
    assert_eq!(
        plan_update(&waiting, Some(&chosen), None),
        Err(PlanError::NeedsUpstream {
            item: result("a"),
            needs: mesh("a"),
        })
    );
}

#[test]
fn an_item_that_is_not_pending_is_refused() {
    let waiting = [result("a")];
    let chosen = [mesh("a")];
    assert_eq!(
        plan_update(&waiting, Some(&chosen), None),
        Err(PlanError::NotPending(mesh("a")))
    );
}

#[test]
fn the_user_may_interleave_documents_where_dependencies_allow() {
    let waiting = [mesh("a"), result("a"), mesh("b"), result("b")];
    let order = [mesh("b"), mesh("a"), result("b"), result("a")];
    assert_eq!(plan_update(&waiting, None, Some(&order)).unwrap(), order);
}

#[test]
fn an_order_that_breaks_a_dependency_is_refused() {
    let waiting = [mesh("a"), result("a")];
    let order = [result("a"), mesh("a")];
    assert_eq!(
        plan_update(&waiting, None, Some(&order)),
        Err(PlanError::OrderBreaksDependency {
            item: result("a"),
            needs: mesh("a"),
        })
    );
}

#[test]
fn an_order_must_list_each_chosen_item_exactly_once() {
    let waiting = [mesh("a"), result("a"), mesh("b")];
    let chosen = [mesh("a"), result("a")];
    let missing = [mesh("a")];
    let extra = [mesh("a"), result("a"), mesh("b")];
    let repeated = [mesh("a"), mesh("a"), result("a")];
    let swapped = [mesh("a"), mesh("b")];
    let orders: [&[Item]; 4] = [&missing, &extra, &repeated, &swapped];
    for order in orders {
        assert_eq!(
            plan_update(&waiting, Some(&chosen), Some(order)),
            Err(PlanError::OrderMismatch),
            "{order:?}"
        );
    }
}

#[test]
fn a_selection_and_an_order_work_together() {
    let waiting = [mesh("a"), result("a"), mesh("b")];
    let chosen = [mesh("b"), mesh("a")];
    let order = [mesh("b"), mesh("a")];
    assert_eq!(
        plan_update(&waiting, Some(&chosen), Some(&order)).unwrap(),
        order
    );
    let short = [mesh("a")];
    assert_eq!(
        plan_update(&waiting, Some(&chosen), Some(&short)),
        Err(PlanError::OrderMismatch)
    );
}

#[test]
fn nothing_pending_is_an_empty_plan() {
    assert!(plan_update(&[], None, None).unwrap().is_empty());
}
