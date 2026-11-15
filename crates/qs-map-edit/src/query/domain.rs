// SPDX-License-Identifier: GPL-3.0-or-later

//! The objects a query searches and their fields, and which object types a query applies to,
//! as TrenchBroom's manual's "Query Language" says: "Object Types", "Fields" and "Which
//! Objects a Query Searches". A query is specialized for each type in turn: a condition on
//! fields the type lacks becomes a constant that cannot make it match, whether negated or not,
//! and one that only reads `type` becomes its value; the constants fold into the `!`, `&&` and
//! `||` around them, and the types for which the query is not folded to anything but `true`
//! are its domain.

use std::collections::BTreeSet;

use super::syntax::{Binary, Expr, Unary};
use super::value::Value;

/// The type of an object a query searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ObjectType {
    /// The world: the `worldspawn` entity.
    World,
    /// A layer, the default layer among them.
    Layer,
    /// A group.
    Group,
    /// A point or brush entity.
    Entity,
    /// A brush.
    Brush,
    /// A brush's face.
    Face,
}

impl ObjectType {
    /// Every type.
    pub(crate) const ALL: [Self; 6] = [
        Self::World,
        Self::Layer,
        Self::Group,
        Self::Entity,
        Self::Brush,
        Self::Face,
    ];

    /// Its name, the value of its objects' `type`.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Layer => "layer",
            Self::Group => "group",
            Self::Entity => "entity",
            Self::Brush => "brush",
            Self::Face => "face",
        }
    }
}

/// The names a query may write without quotes when comparing `type` with them: every type's,
/// and `patch`, TrenchBroom's type for Quake 3's patches, which a Quake map never holds.
pub(crate) const TYPE_NAMES: [&str; 7] = [
    "world", "layer", "group", "entity", "brush", "patch", "face",
];

/// A field of an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Field {
    /// `type`.
    Type,
    /// `name`: a layer's or group's.
    Name,
    /// `classname`.
    Classname,
    /// `properties`: an entity's keys and values.
    Properties,
    /// `entity`: the entity a brush or face belongs to.
    Entity,
    /// `materials`: a brush's textures, each once.
    Materials,
    /// `material`: a face's texture.
    Material,
    /// `normal`: a face's.
    Normal,
    /// `tags`: the smart tags matching the object.
    Tags,
    /// `bounds`.
    Bounds,
    /// `center`.
    Center,
    /// `layerName`.
    LayerName,
    /// `groupName`.
    GroupName,
    /// `visible`.
    Visible,
    /// `locked`.
    Locked,
}

impl Field {
    /// Every field.
    pub(crate) const ALL: [Self; 15] = [
        Self::Type,
        Self::Name,
        Self::Classname,
        Self::Properties,
        Self::Entity,
        Self::Materials,
        Self::Material,
        Self::Normal,
        Self::Tags,
        Self::Bounds,
        Self::Center,
        Self::LayerName,
        Self::GroupName,
        Self::Visible,
        Self::Locked,
    ];

    /// Its name in a query.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Name => "name",
            Self::Classname => "classname",
            Self::Properties => "properties",
            Self::Entity => "entity",
            Self::Materials => "materials",
            Self::Material => "material",
            Self::Normal => "normal",
            Self::Tags => "tags",
            Self::Bounds => "bounds",
            Self::Center => "center",
            Self::LayerName => "layerName",
            Self::GroupName => "groupName",
            Self::Visible => "visible",
            Self::Locked => "locked",
        }
    }

    /// The field of `name`.
    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.name() == name)
    }

    /// The object types that have it, from the manual's table.
    pub(crate) const fn types(self) -> &'static [ObjectType] {
        use ObjectType::{Brush, Entity, Face, Group, Layer, World};
        match self {
            Self::Type | Self::Visible | Self::Locked => &ObjectType::ALL,
            Self::Name => &[Layer, Group],
            Self::Classname | Self::Properties => &[World, Entity],
            Self::Entity => &[Brush, Face],
            Self::Materials => &[Brush],
            Self::Material | Self::Normal => &[Face],
            Self::Tags => &[World, Entity, Brush, Face],
            Self::Bounds | Self::Center | Self::LayerName | Self::GroupName => {
                &[Layer, Group, Entity, Brush, Face]
            }
        }
    }

    /// Whether a text search looks in it: a string, array or map field, but not `type`,
    /// `entity` or one only faces have, since a text search never finds faces.
    pub(crate) const fn searched_as_text(self) -> bool {
        matches!(
            self,
            Self::Name
                | Self::Classname
                | Self::Properties
                | Self::Materials
                | Self::Tags
                | Self::LayerName
                | Self::GroupName
        )
    }

    /// Whether it is a boolean, a query on its own.
    pub(crate) const fn is_boolean(self) -> bool {
        matches!(self, Self::Visible | Self::Locked)
    }
}

/// An object's fields.
pub(crate) trait Fields {
    /// The value of `field`; undefined when the object lacks it.
    fn field(&self, field: Field) -> Value;
}

/// The fields of an object only known by its type.
struct OfType(ObjectType);

impl Fields for OfType {
    fn field(&self, field: Field) -> Value {
        match field {
            Field::Type => Value::Text(self.0.name().to_owned()),
            _ => Value::Undefined,
        }
    }
}

/// Whether a condition is negated an even or an odd number of times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Polarity {
    Positive,
    Negative,
}

impl Expr {
    /// The names the expression reads, the key after a dot not among them.
    fn names(&self, into: &mut BTreeSet<String>) {
        match self {
            Self::Literal(_) => {}
            Self::Name(name) => {
                into.insert(name.clone());
            }
            Self::Array(items) | Self::Call(_, items) | Self::Unknown(_, items) => {
                for item in items {
                    item.names(into);
                }
            }
            Self::Unary(_, operand) | Self::Dot(operand, _) => operand.names(into),
            Self::Binary(_, lhs, rhs) | Self::Index(lhs, rhs) => {
                lhs.names(into);
                rhs.names(into);
            }
        }
    }

    /// The boolean this expression is, when it is one.
    fn boolean(&self) -> Option<bool> {
        match self {
            Self::Literal(Value::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    /// This query specialized for objects of type `ty`.
    pub(crate) fn specialized(&self, ty: ObjectType) -> Self {
        self.specialize(ty, Polarity::Positive)
    }

    fn specialize(&self, ty: ObjectType, polarity: Polarity) -> Self {
        match self {
            Self::Unary(Unary::Not, operand) => {
                let inverted = match polarity {
                    Polarity::Positive => Polarity::Negative,
                    Polarity::Negative => Polarity::Positive,
                };
                let operand = operand.specialize(ty, inverted);
                match operand.boolean() {
                    Some(b) => Self::Literal(Value::Bool(!b)),
                    None => Self::Unary(Unary::Not, Box::new(operand)),
                }
            }
            Self::Binary(op @ (Binary::And | Binary::Or), lhs, rhs) => {
                // `false && x` is false and `true && x` is x; `true || x` is true and
                // `false || x` is x.
                let absorbing = *op == Binary::Or;
                let lhs = lhs.specialize(ty, polarity);
                let rhs = rhs.specialize(ty, polarity);
                match (lhs.boolean(), rhs.boolean()) {
                    (Some(b), _) | (_, Some(b)) if b == absorbing => {
                        Self::Literal(Value::Bool(absorbing))
                    }
                    (Some(_), _) => rhs,
                    (_, Some(_)) => lhs,
                    (None, None) => Self::Binary(*op, Box::new(lhs), Box::new(rhs)),
                }
            }
            _ => self.condition(ty, polarity),
        }
    }

    /// A single condition specialized: a constant that cannot make the query match when `ty`
    /// lacks every field it reads, its value when it only reads `type`, else itself.
    fn condition(&self, ty: ObjectType, polarity: Polarity) -> Self {
        let mut names = BTreeSet::new();
        self.names(&mut names);
        let fields: Vec<Field> = names.iter().filter_map(|n| Field::named(n)).collect();
        let applies = fields.is_empty() || fields.iter().any(|field| field.types().contains(&ty));
        if !applies {
            return Self::Literal(Value::Bool(polarity == Polarity::Negative));
        }
        if names.iter().all(|name| name == Field::Type.name()) {
            return self
                .evaluate(&OfType(ty))
                .map_or_else(|_| self.clone(), Self::Literal);
        }
        self.clone()
    }

    /// Whether this specialized query can still match: it is not a constant but `true`.
    pub(crate) fn can_match(&self) -> bool {
        match self {
            Self::Literal(value) => *value == Value::Bool(true),
            _ => true,
        }
    }
}
