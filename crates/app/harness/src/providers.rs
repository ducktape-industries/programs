//! Routes per provider: the egress services, in order and each with the wire it speaks, that reach
//! a provider's models; the first account to set a provider's route owns it. An agent names its
//! provider and is reached through that route, its window learned per provider and vendor's name.
use ::program::{Error, ExecCtx, QueryCtx, invalid, not_found};
use store::{PageRequest, PageResponse};

use crate::state::{PROVIDER_ROUTES, PROVIDER_WINDOWS};
use crate::{ProviderRouteRow, RouteService, Window, Wire};

pub(crate) struct Reach {
    pub provider: String,
    pub name: String,
    pub services: Vec<RouteService>,
    pub window: Window,
}

impl Reach {
    pub fn label(&self) -> String {
        format!("provider {}'s route", self.provider)
    }
}

pub(crate) fn lowercase_name(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        })
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

pub(crate) fn forget_unreported(ctx: &ExecCtx) -> Result<(), Error> {
    for (key, window) in PROVIDER_WINDOWS.all(ctx)? {
        if window == Window::Unreported {
            PROVIDER_WINDOWS.remove(ctx, &key);
        }
    }
    Ok(())
}

pub(crate) fn learned(
    ctx: &ExecCtx,
    reach: &Reach,
    service: &str,
    window: Window,
    provider: String,
) -> Result<Option<String>, Error> {
    let routed = &reach.provider;
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
