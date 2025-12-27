//! Database module for PostgreSQL operations using SQLx
//! Uses runtime query checking (no compile-time DATABASE_URL needed)

mod models;

pub use models::*;

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::FromRow;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("Record not found")]
    NotFound,
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// Database connection wrapper
#[derive(Clone)]
pub struct Database {
    pool: Arc<PgPool>,
}

impl Database {
    /// Create a new database connection pool
    pub async fn new(database_url: &str) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        
        info!("Database pool created with max 10 connections");
        
        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ==========================================
    // Config Operations
    // ==========================================

    /// Get live trading config
    pub async fn get_config(&self) -> Result<LiveTradingConfig, DbError> {
        let row = sqlx::query(
            r#"
            SELECT
                id, is_enabled, trade_amount, min_profit_threshold,
                max_daily_loss, max_total_loss, start_currency, custom_currencies,
                max_pairs, min_volume_24h_usd, max_cost_min,
                created_at, updated_at, enabled_at, disabled_at
            FROM live_trading_config
            WHERE id = 1
            "#
        )
        .fetch_optional(self.pool())
        .await?;

        match row {
            Some(row) => Ok(LiveTradingConfig::from_row(&row)?),
            None => Ok(LiveTradingConfig::default()),
        }
    }

    /// Update live trading config
    pub async fn update_config(&self, updates: ConfigUpdate) -> Result<LiveTradingConfig, DbError> {
        let row = sqlx::query(
            r#"
            UPDATE live_trading_config
            SET
                trade_amount = COALESCE($1, trade_amount),
                min_profit_threshold = COALESCE($2, min_profit_threshold),
                max_daily_loss = COALESCE($3, max_daily_loss),
                max_total_loss = COALESCE($4, max_total_loss),
                start_currency = COALESCE($5, start_currency),
                max_pairs = COALESCE($6, max_pairs),
                min_volume_24h_usd = COALESCE($7, min_volume_24h_usd),
                max_cost_min = COALESCE($8, max_cost_min),
                updated_at = CURRENT_TIMESTAMP
            WHERE id = 1
            RETURNING
                id, is_enabled, trade_amount, min_profit_threshold,
                max_daily_loss, max_total_loss, start_currency, custom_currencies,
                max_pairs, min_volume_24h_usd, max_cost_min,
                created_at, updated_at, enabled_at, disabled_at
            "#
        )
        .bind(updates.trade_amount)
        .bind(updates.min_profit_threshold)
        .bind(updates.max_daily_loss)
        .bind(updates.max_total_loss)
        .bind(updates.start_currency)
        .bind(updates.max_pairs)
        .bind(updates.min_volume_24h_usd)
        .bind(updates.max_cost_min)
        .fetch_one(self.pool())
        .await?;

        Ok(LiveTradingConfig::from_row(&row)?)
    }

    /// Enable trading
    pub async fn enable_trading(&self) -> Result<LiveTradingConfig, DbError> {
        let row = sqlx::query(
            r#"
            UPDATE live_trading_config
            SET
                is_enabled = TRUE,
                enabled_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = 1
            RETURNING
                id, is_enabled, trade_amount, min_profit_threshold,
                max_daily_loss, max_total_loss, start_currency, custom_currencies,
                max_pairs, min_volume_24h_usd, max_cost_min,
                created_at, updated_at, enabled_at, disabled_at
            "#
        )
        .fetch_one(self.pool())
        .await?;

        Ok(LiveTradingConfig::from_row(&row)?)
    }

    /// Disable trading
    pub async fn disable_trading(&self, _reason: &str) -> Result<LiveTradingConfig, DbError> {
        let row = sqlx::query(
            r#"
            UPDATE live_trading_config
            SET
                is_enabled = FALSE,
                disabled_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = 1
            RETURNING
                id, is_enabled, trade_amount, min_profit_threshold,
                max_daily_loss, max_total_loss, start_currency, custom_currencies,
                max_pairs, min_volume_24h_usd, max_cost_min,
                created_at, updated_at, enabled_at, disabled_at
            "#
        )
        .fetch_one(self.pool())
        .await?;

        Ok(LiveTradingConfig::from_row(&row)?)
    }

    // ==========================================
    // State Operations
    // ==========================================

    /// Get live trading state
    pub async fn get_state(&self) -> Result<LiveTradingState, DbError> {
        let row = sqlx::query(
            r#"
            SELECT 
                id, daily_loss, daily_profit, daily_trades, daily_wins,
                total_loss, total_profit, total_trades, total_wins,
                COALESCE(total_trade_amount, 0.0) as total_trade_amount,
                COALESCE(partial_trades, 0) as partial_trades,
                COALESCE(partial_estimated_loss, 0.0) as partial_estimated_loss,
                COALESCE(partial_estimated_profit, 0.0) as partial_estimated_profit,
                COALESCE(partial_trade_amount, 0.0) as partial_trade_amount,
                is_circuit_broken, circuit_broken_at, circuit_broken_reason,
                last_trade_at, last_daily_reset, is_executing, current_trade_id,
                created_at, updated_at
            FROM live_trading_state
            WHERE id = 1
            "#
        )
        .fetch_optional(self.pool())
        .await?;

        match row {
            Some(row) => Ok(LiveTradingState::from_row(&row)?),
            None => Ok(LiveTradingState::default()),
        }
    }

    /// Trip circuit breaker
    pub async fn trip_circuit_breaker(&self, reason: &str) -> Result<LiveTradingState, DbError> {
        let row = sqlx::query(
            r#"
            UPDATE live_trading_state
            SET
                is_circuit_broken = TRUE,
                circuit_broken_at = CURRENT_TIMESTAMP,
                circuit_broken_reason = $1,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = 1
            RETURNING 
                id, daily_loss, daily_profit, daily_trades, daily_wins,
                total_loss, total_profit, total_trades, total_wins,
                COALESCE(total_trade_amount, 0.0) as total_trade_amount,
                COALESCE(partial_trades, 0) as partial_trades,
                COALESCE(partial_estimated_loss, 0.0) as partial_estimated_loss,
                COALESCE(partial_estimated_profit, 0.0) as partial_estimated_profit,
                COALESCE(partial_trade_amount, 0.0) as partial_trade_amount,
                is_circuit_broken, circuit_broken_at, circuit_broken_reason,
                last_trade_at, last_daily_reset, is_executing, current_trade_id,
                created_at, updated_at
            "#
        )
        .bind(reason)
        .fetch_one(self.pool())
        .await?;

        Ok(LiveTradingState::from_row(&row)?)
    }

    /// Reset circuit breaker
    pub async fn reset_circuit_breaker(&self) -> Result<LiveTradingState, DbError> {
        let row = sqlx::query(
            r#"
            UPDATE live_trading_state
            SET
                is_circuit_broken = FALSE,
                circuit_broken_at = NULL,
                circuit_broken_reason = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = 1
            RETURNING 
                id, daily_loss, daily_profit, daily_trades, daily_wins,
                total_loss, total_profit, total_trades, total_wins,
                COALESCE(total_trade_amount, 0.0) as total_trade_amount,
                COALESCE(partial_trades, 0) as partial_trades,
                COALESCE(partial_estimated_loss, 0.0) as partial_estimated_loss,
                COALESCE(partial_estimated_profit, 0.0) as partial_estimated_profit,
                COALESCE(partial_trade_amount, 0.0) as partial_trade_amount,
                is_circuit_broken, circuit_broken_at, circuit_broken_reason,
                last_trade_at, last_daily_reset, is_executing, current_trade_id,
                created_at, updated_at
            "#
        )
        .fetch_one(self.pool())
        .await?;

        Ok(LiveTradingState::from_row(&row)?)
    }

    /// Reset daily stats
    pub async fn reset_daily_stats(&self) -> Result<LiveTradingState, DbError> {
        let row = sqlx::query(
            r#"
            UPDATE live_trading_state
            SET
                daily_loss = 0.0,
                daily_profit = 0.0,
                daily_trades = 0,
                daily_wins = 0,
                last_daily_reset = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = 1
            RETURNING 
                id, daily_loss, daily_profit, daily_trades, daily_wins,
                total_loss, total_profit, total_trades, total_wins,
                COALESCE(total_trade_amount, 0.0) as total_trade_amount,
                COALESCE(partial_trades, 0) as partial_trades,
                COALESCE(partial_estimated_loss, 0.0) as partial_estimated_loss,
                COALESCE(partial_estimated_profit, 0.0) as partial_estimated_profit,
                COALESCE(partial_trade_amount, 0.0) as partial_trade_amount,
                is_circuit_broken, circuit_broken_at, circuit_broken_reason,
                last_trade_at, last_daily_reset, is_executing, current_trade_id,
                created_at, updated_at
            "#
        )
        .fetch_one(self.pool())
        .await?;

        Ok(LiveTradingState::from_row(&row)?)
    }

    /// Record a completed trade result in the state
    pub async fn record_trade_result(
        &self,
        profit_loss: f64,
        trade_amount: f64,
        is_win: bool,
    ) -> Result<(), DbError> {
        // Update based on whether it was a profit or loss
        if profit_loss >= 0.0 {
            sqlx::query(
                r#"
                UPDATE live_trading_state
                SET
                    daily_profit = daily_profit + $1,
                    total_profit = total_profit + $1,
                    daily_trades = daily_trades + 1,
                    total_trades = total_trades + 1,
                    daily_wins = daily_wins + CASE WHEN $2 THEN 1 ELSE 0 END,
                    total_wins = total_wins + CASE WHEN $2 THEN 1 ELSE 0 END,
                    total_trade_amount = COALESCE(total_trade_amount, 0) + $3,
                    last_trade_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = 1
                "#
            )
            .bind(profit_loss)
            .bind(is_win)
            .bind(trade_amount)
            .execute(self.pool())
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE live_trading_state
                SET
                    daily_loss = daily_loss + $1,
                    total_loss = total_loss + $1,
                    daily_trades = daily_trades + 1,
                    total_trades = total_trades + 1,
                    total_trade_amount = COALESCE(total_trade_amount, 0) + $2,
                    last_trade_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = 1
                "#
            )
            .bind(profit_loss.abs())
            .bind(trade_amount)
            .execute(self.pool())
            .await?;
        }
        Ok(())
    }

    // ==========================================
    // Trade Operations
    // ==========================================

    /// Save a new trade
    pub async fn save_trade(&self, trade: &NewLiveTrade) -> Result<LiveTrade, DbError> {
        let row = sqlx::query(
            r#"
            INSERT INTO live_trades (
                trade_id, path, legs, amount_in, amount_out,
                profit_loss, profit_loss_pct, status, current_leg,
                error_message, held_currency, held_amount, held_value_usd,
                order_ids, leg_fills, started_at, completed_at,
                total_execution_ms, opportunity_profit_pct, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, COALESCE($16, NOW()), $17, $18, $19, NOW())
            RETURNING
                id, trade_id, path, legs, amount_in, amount_out,
                profit_loss, profit_loss_pct, status, current_leg,
                error_message, held_currency, held_amount, held_value_usd,
                resolved_at AT TIME ZONE 'UTC' as resolved_at,
                resolved_amount_usd, resolution_trade_id,
                order_ids, leg_fills,
                started_at AT TIME ZONE 'UTC' as started_at,
                completed_at AT TIME ZONE 'UTC' as completed_at,
                total_execution_ms, opportunity_profit_pct,
                created_at AT TIME ZONE 'UTC' as created_at
            "#
        )
        .bind(&trade.trade_id)
        .bind(&trade.path)
        .bind(trade.legs)
        .bind(trade.amount_in)
        .bind(trade.amount_out)
        .bind(trade.profit_loss)
        .bind(trade.profit_loss_pct)
        .bind(&trade.status)
        .bind(trade.current_leg)
        .bind(&trade.error_message)
        .bind(&trade.held_currency)
        .bind(trade.held_amount)
        .bind(trade.held_value_usd)
        .bind(&trade.order_ids)
        .bind(&trade.leg_fills)
        .bind(trade.started_at)
        .bind(trade.completed_at)
        .bind(trade.total_execution_ms)
        .bind(trade.opportunity_profit_pct)
        .fetch_one(self.pool())
        .await?;

        Ok(LiveTrade::from_row(&row)?)
    }

    /// Get trades with filters
    pub async fn get_trades(&self, limit: i64, status: Option<&str>, hours: i32) -> Result<Vec<LiveTrade>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, trade_id, path, legs, amount_in, amount_out,
                profit_loss, profit_loss_pct, status, current_leg,
                error_message, held_currency, held_amount, held_value_usd,
                resolved_at AT TIME ZONE 'UTC' as resolved_at,
                resolved_amount_usd, resolution_trade_id,
                order_ids, leg_fills,
                started_at AT TIME ZONE 'UTC' as started_at,
                completed_at AT TIME ZONE 'UTC' as completed_at,
                total_execution_ms, opportunity_profit_pct,
                created_at AT TIME ZONE 'UTC' as created_at
            FROM live_trades
            WHERE
                ($1::text IS NULL OR status = $1)
                AND (created_at IS NULL OR created_at > NOW() - make_interval(hours => $2))
            ORDER BY id DESC
            LIMIT $3
            "#
        )
        .bind(status)
        .bind(hours)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;

        let mut trades = Vec::new();
        for row in rows {
            trades.push(LiveTrade::from_row(&row)?);
        }
        Ok(trades)
    }

    /// Get trades count for pagination
    pub async fn get_trades_count(&self, status: Option<&str>, hours: i32) -> Result<i64, DbError> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM live_trades
            WHERE
                ($1::text IS NULL OR status = $1)
                AND (created_at IS NULL OR created_at > NOW() - make_interval(hours => $2))
            "#
        )
        .bind(status)
        .bind(hours)
        .fetch_one(self.pool())
        .await?;

        Ok(row.0)
    }

    /// Get trades with pagination (limit + offset)
    pub async fn get_trades_paginated(&self, limit: i64, offset: i64, status: Option<&str>, hours: i32) -> Result<Vec<LiveTrade>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, trade_id, path, legs, amount_in, amount_out,
                profit_loss, profit_loss_pct, status, current_leg,
                error_message, held_currency, held_amount, held_value_usd,
                resolved_at AT TIME ZONE 'UTC' as resolved_at,
                resolved_amount_usd, resolution_trade_id,
                order_ids, leg_fills,
                started_at AT TIME ZONE 'UTC' as started_at,
                completed_at AT TIME ZONE 'UTC' as completed_at,
                total_execution_ms, opportunity_profit_pct,
                created_at AT TIME ZONE 'UTC' as created_at
            FROM live_trades
            WHERE
                ($1::text IS NULL OR status = $1)
                AND (created_at IS NULL OR created_at > NOW() - make_interval(hours => $2))
            ORDER BY id DESC
            LIMIT $3 OFFSET $4
            "#
        )
        .bind(status)
        .bind(hours)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool())
        .await?;

        let mut trades = Vec::new();
        for row in rows {
            trades.push(LiveTrade::from_row(&row)?);
        }
        Ok(trades)
    }

    /// Get a single trade by ID
    pub async fn get_trade(&self, trade_id: &str) -> Result<Option<LiveTrade>, DbError> {
        let row = sqlx::query(
            r#"
            SELECT 
                id, trade_id, path, legs, amount_in, amount_out,
                profit_loss, profit_loss_pct, status, current_leg,
                error_message, held_currency, held_amount, held_value_usd,
                resolved_at, resolved_amount_usd, resolution_trade_id,
                order_ids, leg_fills, started_at, completed_at,
                total_execution_ms, opportunity_profit_pct, created_at
            FROM live_trades
            WHERE trade_id = $1
            "#
        )
        .bind(trade_id)
        .fetch_optional(self.pool())
        .await?;

        match row {
            Some(row) => Ok(Some(LiveTrade::from_row(&row)?)),
            None => Ok(None),
        }
    }

    /// Update trade status
    pub async fn update_trade_status(
        &self,
        trade_id: &str,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<LiveTrade, DbError> {
        let row = sqlx::query(
            r#"
            UPDATE live_trades
            SET 
                status = $2,
                error_message = $3,
                completed_at = CASE WHEN $2 IN ('COMPLETED', 'FAILED', 'RESOLVED') THEN CURRENT_TIMESTAMP ELSE completed_at END
            WHERE trade_id = $1
            RETURNING 
                id, trade_id, path, legs, amount_in, amount_out,
                profit_loss, profit_loss_pct, status, current_leg,
                error_message, held_currency, held_amount, held_value_usd,
                resolved_at, resolved_amount_usd, resolution_trade_id,
                order_ids, leg_fills, started_at, completed_at,
                total_execution_ms, opportunity_profit_pct, created_at
            "#
        )
        .bind(trade_id)
        .bind(status)
        .bind(error_message)
        .fetch_one(self.pool())
        .await?;

        Ok(LiveTrade::from_row(&row)?)
    }

    /// Resolve a partial trade - update trade with resolution details and update state
    pub async fn resolve_partial_trade(
        &self,
        trade_id: &str,
        resolved_amount_usd: f64,
        original_amount: f64,
    ) -> Result<LiveTrade, DbError> {
        let profit_loss = resolved_amount_usd - original_amount;
        let profit_loss_pct = if original_amount > 0.0 {
            (profit_loss / original_amount) * 100.0
        } else {
            0.0
        };

        // Update the trade record
        let row = sqlx::query(
            r#"
            UPDATE live_trades
            SET 
                status = 'RESOLVED',
                amount_out = $2,
                profit_loss = $3,
                profit_loss_pct = $4,
                resolved_at = CURRENT_TIMESTAMP,
                resolved_amount_usd = $2,
                completed_at = CURRENT_TIMESTAMP
            WHERE trade_id = $1
            RETURNING 
                id, trade_id, path, legs, amount_in, amount_out,
                profit_loss, profit_loss_pct, status, current_leg,
                error_message, held_currency, held_amount, held_value_usd,
                resolved_at, resolved_amount_usd, resolution_trade_id,
                order_ids, leg_fills, started_at, completed_at,
                total_execution_ms, opportunity_profit_pct, created_at
            "#
        )
        .bind(trade_id)
        .bind(resolved_amount_usd)
        .bind(profit_loss)
        .bind(profit_loss_pct)
        .fetch_one(self.pool())
        .await?;

        // Update state - decrement partial, add to totals
        sqlx::query(
            r#"
            UPDATE live_trading_state
            SET 
                partial_trades = GREATEST(0, partial_trades - 1),
                partial_estimated_loss = GREATEST(0, partial_estimated_loss - ABS($2)),
                partial_trade_amount = GREATEST(0, partial_trade_amount - $3),
                total_trades = total_trades + 1,
                total_profit = CASE WHEN $2 >= 0 THEN total_profit + $2 ELSE total_profit END,
                total_loss = CASE WHEN $2 < 0 THEN total_loss + ABS($2) ELSE total_loss END,
                total_wins = CASE WHEN $2 >= 0 THEN total_wins + 1 ELSE total_wins END,
                daily_trades = daily_trades + 1,
                daily_profit = CASE WHEN $2 >= 0 THEN daily_profit + $2 ELSE daily_profit END,
                daily_loss = CASE WHEN $2 < 0 THEN daily_loss + ABS($2) ELSE daily_loss END,
                daily_wins = CASE WHEN $2 >= 0 THEN daily_wins + 1 ELSE daily_wins END,
                last_trade_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = 1
            "#
        )
        .bind(profit_loss)
        .bind(profit_loss)
        .bind(original_amount)
        .execute(self.pool())
        .await?;

        Ok(LiveTrade::from_row(&row)?)
    }

    // ==========================================
    // Opportunity Operations
    // ==========================================

    /// Save a profitable opportunity
    pub async fn save_opportunity(&self, opp: &NewLiveOpportunity) -> Result<LiveOpportunity, DbError> {
        let row = sqlx::query(
            r#"
            INSERT INTO live_opportunities (
                path, legs, expected_profit_pct, expected_profit_usd,
                trade_amount, status, status_reason, pairs_scanned, paths_found
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING 
                id, found_at, path, legs, expected_profit_pct, expected_profit_usd,
                trade_amount, status, status_reason, trade_id, pairs_scanned, paths_found,
                created_at, updated_at
            "#
        )
        .bind(&opp.path)
        .bind(opp.legs)
        .bind(opp.expected_profit_pct)
        .bind(opp.expected_profit_usd)
        .bind(opp.trade_amount)
        .bind(&opp.status)
        .bind(&opp.status_reason)
        .bind(opp.pairs_scanned)
        .bind(opp.paths_found)
        .fetch_one(self.pool())
        .await?;

        Ok(LiveOpportunity::from_row(&row)?)
    }

    /// Get opportunities with filters
    pub async fn get_opportunities(&self, limit: i64, status: Option<&str>, hours: i32) -> Result<Vec<LiveOpportunity>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT 
                id, found_at, path, legs, expected_profit_pct, expected_profit_usd,
                trade_amount, status, status_reason, trade_id, pairs_scanned, paths_found,
                created_at, updated_at
            FROM live_opportunities
            WHERE 
                ($1::text IS NULL OR status = $1)
                AND found_at > NOW() - make_interval(hours => $2)
            ORDER BY found_at DESC
            LIMIT $3
            "#
        )
        .bind(status)
        .bind(hours)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;

        let mut opportunities = Vec::new();
        for row in rows {
            opportunities.push(LiveOpportunity::from_row(&row)?);
        }
        Ok(opportunities)
    }

    /// Update opportunity status (e.g., when executed)
    pub async fn update_opportunity_status(
        &self,
        opp_id: i32,
        status: &str,
        trade_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            r#"
            UPDATE live_opportunities
            SET 
                status = $2,
                trade_id = COALESCE($3, trade_id),
                status_reason = COALESCE($4, status_reason),
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $1
            "#
        )
        .bind(opp_id)
        .bind(status)
        .bind(trade_id)
        .bind(reason)
        .execute(self.pool())
        .await?;

        Ok(())
    }

    /// Clean old opportunities (keep last 7 days)
    pub async fn clean_old_opportunities(&self) -> Result<u64, DbError> {
        let result = sqlx::query(
            r#"
            DELETE FROM live_opportunities
            WHERE found_at < NOW() - INTERVAL '7 days'
            "#
        )
        .execute(self.pool())
        .await?;

        Ok(result.rows_affected())
    }

    // ==========================================
    // Fee Configuration Operations
    // ==========================================

    /// Get fee configuration
    pub async fn get_fee_configuration(&self) -> Result<FeeConfiguration, DbError> {
        let row = sqlx::query(
            r#"
            SELECT
                id, maker_fee, taker_fee, fee_source, volume_tier,
                thirty_day_volume, last_fetched_at, last_updated_at, created_at
            FROM fee_configuration
            WHERE id = 1
            "#
        )
        .fetch_optional(self.pool())
        .await?;

        match row {
            Some(row) => Ok(FeeConfiguration::from_row(&row)?),
            None => Ok(FeeConfiguration::default()),
        }
    }

    /// Update fee configuration from Kraken API
    pub async fn update_fee_from_kraken(
        &self,
        maker_fee: f64,
        taker_fee: f64,
        volume_tier: Option<&str>,
        thirty_day_volume: Option<f64>,
    ) -> Result<FeeConfiguration, DbError> {
        let row = sqlx::query(
            r#"
            INSERT INTO fee_configuration (id, maker_fee, taker_fee, fee_source, volume_tier, thirty_day_volume, last_fetched_at)
            VALUES (1, $1, $2, 'kraken_api', $3, $4, CURRENT_TIMESTAMP)
            ON CONFLICT (id) DO UPDATE SET
                maker_fee = $1,
                taker_fee = $2,
                fee_source = 'kraken_api',
                volume_tier = $3,
                thirty_day_volume = $4,
                last_fetched_at = CURRENT_TIMESTAMP,
                last_updated_at = CURRENT_TIMESTAMP
            RETURNING
                id, maker_fee, taker_fee, fee_source, volume_tier,
                thirty_day_volume, last_fetched_at, last_updated_at, created_at
            "#
        )
        .bind(maker_fee)
        .bind(taker_fee)
        .bind(volume_tier)
        .bind(thirty_day_volume)
        .fetch_one(self.pool())
        .await?;

        Ok(FeeConfiguration::from_row(&row)?)
    }

    /// Update fee configuration manually
    pub async fn update_fee_manual(
        &self,
        maker_fee: f64,
        taker_fee: f64,
    ) -> Result<FeeConfiguration, DbError> {
        let row = sqlx::query(
            r#"
            INSERT INTO fee_configuration (id, maker_fee, taker_fee, fee_source)
            VALUES (1, $1, $2, 'manual')
            ON CONFLICT (id) DO UPDATE SET
                maker_fee = $1,
                taker_fee = $2,
                fee_source = 'manual',
                last_updated_at = CURRENT_TIMESTAMP
            RETURNING
                id, maker_fee, taker_fee, fee_source, volume_tier,
                thirty_day_volume, last_fetched_at, last_updated_at, created_at
            "#
        )
        .bind(maker_fee)
        .bind(taker_fee)
        .fetch_one(self.pool())
        .await?;

        Ok(FeeConfiguration::from_row(&row)?)
    }

    /// Check if fees are configured (not pending)
    pub async fn are_fees_configured(&self) -> Result<bool, DbError> {
        let fee_config = self.get_fee_configuration().await?;
        Ok(fee_config.fee_source != "pending")
    }
}