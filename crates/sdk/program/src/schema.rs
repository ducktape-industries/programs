//! A program's interface as one borsh schema: its op, query and reply, and what each op returns,
//! under the named fields of [`Interface`]. A founding packs it as the [`SECTION`] custom section.
use std::collections::BTreeMap;
use std::marker::PhantomData;

use borsh::BorshSchema;
use borsh::schema::{BorshSchemaContainer, Declaration, Definition, Fields, add_definition};

use crate::Program;

pub const SECTION: &str = "ducktape.schema";

#[derive(BorshSchema)]
pub struct Interface<O, Q, R, T> {
    pub op: O,
    pub query: Q,
    pub reply: R,
    pub returns: T,
}

// One op variant and the type its output is the borsh of, as `Program::RETURNS` lists them.
#[derive(Clone, Copy)]
pub struct Return {
    pub op: &'static str,
    pub of: fn(&mut BTreeMap<Declaration, Definition>) -> Declaration,
}

pub const fn returns<T: BorshSchema>(op: &'static str) -> Return {
    Return {
        op,
        of: declared::<T>,
    }
}

fn declared<T: BorshSchema>(definitions: &mut BTreeMap<Declaration, Definition>) -> Declaration {
    T::add_definitions_recursively(definitions);
    T::declaration()
}

// A struct with a field per op variant `P::RETURNS` names, of the type that op returns.
pub struct Returns<P>(PhantomData<P>);

impl<P: Program> BorshSchema for Returns<P>
where
    P::Op: BorshSchema,
{
    fn add_definitions_recursively(definitions: &mut BTreeMap<Declaration, Definition>) {
        P::Op::add_definitions_recursively(definitions);
        let variants = variants(definitions, &P::Op::declaration());
        let mut fields: Vec<(String, Declaration)> = Vec::new();
        for Return { op, of } in P::RETURNS {
            let program = P::NAME;
            let named = variants.iter().any(|variant| variant == op);
            assert!(
                named,
                "{program} says what its op {op} returns, and its Op has no variant {op}"
            );
            let again = fields.iter().any(|(field, _)| field == op);
            assert!(!again, "{program} says what its op {op} returns twice");
            fields.push(((*op).to_owned(), of(definitions)));
        }
        let definition = Definition::Struct {
            fields: Fields::NamedFields(fields),
        };
        add_definition(Self::declaration(), definition, definitions);
    }

    fn declaration() -> Declaration {
        format!("Returns<{}>", P::Op::declaration())
    }
}

fn variants(definitions: &BTreeMap<Declaration, Definition>, op: &str) -> Vec<String> {
    match definitions.get(op) {
        Some(Definition::Enum { variants, .. }) => {
            variants.iter().map(|(_, name, _)| name.clone()).collect()
        }
        _ => Vec::new(),
    }
}

pub fn interface<P: Program>() -> BorshSchemaContainer
where
    P::Op: BorshSchema,
    P::Query: BorshSchema,
    P::Reply: BorshSchema,
{
    BorshSchemaContainer::for_type::<Interface<P::Op, P::Query, P::Reply, Returns<P>>>()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Roots<'s> {
    pub op: &'s str,
    pub query: &'s str,
    pub reply: &'s str,
    // none in a schema packed before its interface named what ops return
    pub returns: Option<&'s str>,
}

pub fn roots(schema: &BorshSchemaContainer) -> Option<Roots<'_>> {
    let Some(Definition::Struct {
        fields: Fields::NamedFields(fields),
    }) = schema.get_definition(schema.declaration())
    else {
        return None;
    };
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, declaration)| declaration.as_str())
    };
    Some(Roots {
        op: field("op")?,
        query: field("query")?,
        reply: field("reply")?,
        returns: field("returns"),
    })
}

// Each op variant `returns` (a schema's `Roots::returns`) names, with what it returns.
pub fn returned<'s>(
    schema: &'s BorshSchemaContainer,
    returns: &str,
) -> Option<Vec<(&'s str, &'s str)>> {
    match schema.get_definition(returns)? {
        Definition::Struct {
            fields: Fields::NamedFields(fields),
        } => Some(
            fields
                .iter()
                .map(|(op, declaration)| (op.as_str(), declaration.as_str()))
                .collect(),
        ),
        Definition::Struct {
            fields: Fields::Empty,
        } => Some(Vec::new()),
        _ => None,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use abi::role::scheduler::Op;

    use super::*;
    use crate::{Error, QueryCtx};

    struct Timer;

    impl Program for Timer {
        const NAME: &'static str = "timer";
        type Op = Op;
        type Query = ();
        type Reply = ();
        const RETURNS: &'static [Return] = &[returns::<u64>("Schedule")];

        fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }
    }

    struct Quiet;

    impl Program for Quiet {
        const NAME: &'static str = "quiet";
        type Op = Op;
        type Query = ();
        type Reply = ();

        fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }
    }

    struct Misnamed;

    impl Program for Misnamed {
        const NAME: &'static str = "misnamed";
        type Op = Op;
        type Query = ();
        type Reply = ();
        const RETURNS: &'static [Return] = &[returns::<u64>("Shedule")];

        fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }
    }

    struct Twice;

    impl Program for Twice {
        const NAME: &'static str = "twice";
        type Op = Op;
        type Query = ();
        type Reply = ();
        const RETURNS: &'static [Return] = &[returns::<u64>("Cancel"), returns::<()>("Cancel")];

        fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn the_interface_names_what_each_declared_op_returns_and_no_other() {
        let schema = interface::<Timer>();
        schema.validate().unwrap();
        let named = roots(&schema).unwrap();
        assert_eq!(
            (named.op, named.query, named.reply, named.returns),
            ("Op", "()", "()", Some("Returns<Op>"))
        );
        assert_eq!(
            returned(&schema, "Returns<Op>"),
            Some(vec![("Schedule", "u64")])
        );

        let quiet = interface::<Quiet>();
        quiet.validate().unwrap();
        let returns = roots(&quiet).unwrap().returns.unwrap();
        assert_eq!(returned(&quiet, returns), Some(Vec::new()));
    }

    #[test]
    #[should_panic(
        expected = "misnamed says what its op Shedule returns, and its Op has no variant Shedule"
    )]
    fn a_return_for_no_variant_of_the_op_fails_the_interface() {
        interface::<Misnamed>();
    }

    #[test]
    #[should_panic(expected = "twice says what its op Cancel returns twice")]
    fn a_return_said_twice_fails_the_interface() {
        interface::<Twice>();
    }
}
