//! All route-parameters registered to pilatus-web must implement DependencyProvider
//! In most cases, it simply returns (), which represents "no dependencies", except for
//! - Tuples of DependencyProvider
//! - Inject<Dependency>
//!
//! Because rust doesn't have impl-specialization yet (https://rust-lang.github.io/rfcs/1210-impl-specialization.html)
//! every axum-extractor must have it's own implementation for DependencyProvider

use std::sync::Arc;

use axum::http::HeaderMap;
use bytes::Bytes;
use minfac::{Registered, Resolvable};

use super::{
    extract::{
        ws::WebSocketUpgrade, Abort, Body, Inject, InjectAll, InjectRegistered, Json, Path, Query,
    },
    ws::WebSocketDropperService,
    AbortServiceInterface,
};

pub trait DependencyProvider {
    type Dep: Resolvable;
}

pub trait RecursiveDependencyProvider: DependencyProvider {}

impl<T: Resolvable> DependencyProvider for Inject<T> {
    type Dep = T;
}
impl<T: Resolvable> RecursiveDependencyProvider for Inject<T> {}

impl<T: std::any::Any> DependencyProvider for InjectRegistered<T> {
    type Dep = Registered<T>;
}
impl<T: std::any::Any> RecursiveDependencyProvider for InjectRegistered<T> {}

impl DependencyProvider for Abort {
    type Dep = Registered<AbortServiceInterface>;
}
impl RecursiveDependencyProvider for Abort {}

impl DependencyProvider for WebSocketUpgrade {
    type Dep = Registered<Arc<dyn WebSocketDropperService>>;
}
impl RecursiveDependencyProvider for WebSocketUpgrade {}

impl<T: std::any::Any> DependencyProvider for InjectAll<T> {
    type Dep = ();
}
impl<T: std::any::Any> RecursiveDependencyProvider for InjectAll<T> {}

// Native Axum
impl<T> DependencyProvider for Json<T> {
    type Dep = ();
}
impl<T> RecursiveDependencyProvider for Json<T> {}

impl DependencyProvider for Body {
    type Dep = ();
}
impl RecursiveDependencyProvider for Body {}

impl DependencyProvider for HeaderMap {
    type Dep = ();
}
impl RecursiveDependencyProvider for HeaderMap {}

impl<T> DependencyProvider for Path<T> {
    type Dep = ();
}
impl<T> RecursiveDependencyProvider for Path<T> {}

impl DependencyProvider for Bytes {
    type Dep = ();
}
impl RecursiveDependencyProvider for Bytes {}

impl DependencyProvider for String {
    type Dep = ();
}
impl RecursiveDependencyProvider for String {}

impl<T> DependencyProvider for Query<T> {
    type Dep = ();
}
impl<T> RecursiveDependencyProvider for Query<T> {}

// Tuples
impl DependencyProvider for () {
    type Dep = ();
}
impl RecursiveDependencyProvider for () {}

impl<T: RecursiveDependencyProvider> DependencyProvider for (T,) {
    type Dep = ();
}

impl<FromPartsOrViaRequest, T1: RecursiveDependencyProvider> DependencyProvider
    for (FromPartsOrViaRequest, T1)
{
    type Dep = T1::Dep;
}

impl<FromPartsOrViaRequest, T1: RecursiveDependencyProvider, T2: RecursiveDependencyProvider>
    DependencyProvider for (FromPartsOrViaRequest, T1, T2)
{
    type Dep = (T1::Dep, T2::Dep);
}

impl<
        FromPartsOrViaRequest,
        T1: RecursiveDependencyProvider,
        T2: RecursiveDependencyProvider,
        T3: RecursiveDependencyProvider,
    > DependencyProvider for (FromPartsOrViaRequest, T1, T2, T3)
{
    type Dep = (T1::Dep, T2::Dep, T3::Dep);
}

impl<
        FromPartsOrViaRequest,
        T1: RecursiveDependencyProvider,
        T2: RecursiveDependencyProvider,
        T3: RecursiveDependencyProvider,
        T4: RecursiveDependencyProvider,
    > DependencyProvider for (FromPartsOrViaRequest, T1, T2, T3, T4)
{
    type Dep = (T1::Dep, T2::Dep, T3::Dep, T4::Dep);
}

pub struct DepPair<T1, T2>(T1, T2);
impl<T1: RecursiveDependencyProvider, T2: RecursiveDependencyProvider> DependencyProvider
    for DepPair<T1, T2>
{
    type Dep = (T1::Dep, T2::Dep);
}
impl<T1: RecursiveDependencyProvider, T2: RecursiveDependencyProvider> RecursiveDependencyProvider
    for DepPair<T1, T2>
{
}

impl<
        FromPartsOrViaRequest,
        T1: RecursiveDependencyProvider,
        T2: RecursiveDependencyProvider,
        T3: RecursiveDependencyProvider,
        T4: RecursiveDependencyProvider,
        T5: RecursiveDependencyProvider,
    > DependencyProvider for (FromPartsOrViaRequest, T1, T2, T3, T4, T5)
{
    type Dep = (
        T1::Dep,
        T2::Dep,
        T3::Dep,
        <DepPair<T4, T5> as DependencyProvider>::Dep,
    );
}
