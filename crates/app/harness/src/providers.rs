//! Routes per provider: the egress services, in order and each with the wire it speaks, that reach
//! a provider's models; the first account to set a provider's route owns it. An agent that names
//! its provider is reached through that route, its window learned per provider and vendor's name.
use ::program::{Error, ExecCtx, QueryCtx, invalid, not_found};
use store::{PageRequest, PageResponse};

use crate::models::lowercase_name;
use crate::state::{PROVIDER_ROUTES, PROVIDER_WINDOWS, PROVIDERS};
use crate::{ModelRow, ProviderRouteRow, RouteService, Window, Wire};

pub(crate) enum Origin {
    Record(ModelRow),
    Route(String),
}

pub(crate) struct Reach {
    pub origin: Origin,
    pub provider: Option<String>,
    pub name: String,
    pub services: Vec<RouteService>,
    pub window: Window,
    pub compact_at: Option<u64>,
}

impl Reach {
    pub fn id(&self) -> String {
        match &self.origin {
            Origin::Record(row) => row.id.clone(),
            Origin::Route(_) => self.name.clone(),
        }
    }

    pub fn label(&self) -> String {
        match &self.origin {
            Origin::Record(row) => format!("model {}", row.id),
            Origin::Route(provider) => format!("provider {provider}'s route"),
        }
    }
}

pub(crate) fn of_record(ctx: &QueryCtx, row: ModelRow) -> Result<Reach, Error> {
    let services = crate::routes::services_of(ctx, &row.id, &row.service)?
        .into_iter()
        .map(|service| RouteService {
            service,
            wire: row.wire,
        })
        .collect();
    let provider = match row.wire {
        Wire::AnthropicMessages => Some(model_wire::anthropic::PROVIDER.to_owned()),
        Wire::Harness => PROVIDERS.get(ctx, &row.id)?,
    };
    Ok(Reach {
        provider,
        name: row.name.clone(),
        services,
        window: row.window,
        compact_at: row.compact_at,
        origin: Origin::Record(row),
    })
}

pub(crate) fn of_route(
    provider: String,
    name: &str,
    services: Vec<RouteService>,
    window: Window,
) -> Reach {
    Reach {
        origin: Origin::Route(provider.clone()),
        provider: Some(provider),
        name: name.to_owned(),
        services,
        window,
        compact_at: None,
    }
}

pub(crate) fn route(ctx: &QueryCtx, provider: &str) -> Result<Option<ProviderRouteRow>, Error> {
    PROVIDER_ROUTES.get(ctx, &provider.to_owned())
}

pub(crate) fn routes(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<ProviderRouteRow>, Error> {
    Ok(PROVIDER_ROUTES.range(ctx, page)?.map(|(_, row)| row))
}

pub(crate) fn window(ctx: &QueryCtx, provider: &str, name: &str) -> Result<Window, Error> {
    let key = (provider.to_owned(), name.to_owned());
    Ok(PROVIDER_WINDOWS.get(ctx, &key)?.unwrap_or(Window::Unasked))
}

pub(crate) fn learned(
    ctx: &ExecCtx,
    reach: &Reach,
    service: &str,
    window: Window,
    provider: String,
) -> Result<Option<String>, Error> {
    let routed = match &reach.origin {
        Origin::Record(row) => {
            crate::models::learned(ctx, row, window, provider)?;
            return Ok(None);
        }
        Origin::Route(routed) => routed,
    };
    if provider != *routed {
        return Ok(Some(format!(
            "service {service} answers for provider {provider}, not {routed}: provider {routed}'s route lists a service that reaches another provider"
        )));
    }
    PROVIDER_WINDOWS.put(ctx, &(routed.clone(), reach.name.clone()), &window);
    Ok(None)
}

pub(crate) fn set_route(
    ctx: &ExecCtx,
    provider: String,
    services: Vec<RouteService>,
) -> Result<(), Error> {
    if !lowercase_name(&provider) {
        return Err(invalid(
            "a provider is lowercase letters, digits, -, _ and .",
        ));
    }
    let anthropic = provider == model_wire::anthropic::PROVIDER;
    let mut listed: Vec<RouteService> = Vec::new();
    for entry in services {
        let reserved = abi::role::egress::RESERVED.contains(&entry.service.as_str());
        if !lowercase_name(&entry.service) || reserved {
            return Err(invalid(
                "a provider is reached through registered egress services, not http or relay",
            ));
        }
        if entry.wire == Wire::AnthropicMessages && !anthropic {
            return Err(invalid(format!(
                "the Anthropic Messages wire reaches anthropic only, not {provider}"
            )));
        }
        if !listed.iter().any(|kept| kept.service == entry.service) {
            listed.push(entry);
        }
    }
    if listed.is_empty() {
        return Err(invalid(
            "a provider is reached through at least one service",
        ));
    }
    let owner = match PROVIDER_ROUTES.get(ctx, &provider)? {
        Some(row) => row.owner,
        None => ctx.sender_account()?,
    };
    let row = ProviderRouteRow {
        provider: provider.clone(),
        owner,
        services: listed,
    };
    PROVIDER_ROUTES.put(ctx, &provider, &row);
    Ok(())
}

pub(crate) fn remove_route(ctx: &ExecCtx, provider: String) -> Result<(), Error> {
    if !PROVIDER_ROUTES.has(ctx, &provider) {
        return Err(not_found(format!("no route for provider {provider}")));
    }
    PROVIDER_ROUTES.remove(ctx, &provider);
    Ok(())
}
